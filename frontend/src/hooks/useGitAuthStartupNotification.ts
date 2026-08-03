import { useEffect, useRef } from "react";

import {
  isTransientGitHubConnectionState,
  requiresGitHubCredentialRepair,
  type GitHubConnectionStatus,
} from "@/api/github";
import { useGitHubConnectionStatus } from "@/hooks/useGitHubConnectionStatus";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import {
  useGitAuthDiagnostics,
  useResumeDeferredGitStartup,
} from "@/hooks/useGithubSettings";
import { selectActiveProject, useProjectStore } from "@/stores/projectStore";
import type { GitAuthDiagnostics } from "@/hooks/useGithubSettings";
import type { Project } from "@/types/project";

function isGithubHttpsRemote(url: string | null | undefined) {
  return url?.trim().startsWith("https://github.com/") ?? false;
}

export function hasStartupGitAuthIssue(
  project: Project | null,
  diagnostics: GitAuthDiagnostics | undefined,
  ghStatus: GitHubConnectionStatus | undefined,
  diagnosticsFailed = false,
) {
  if (!project) {
    return false;
  }
  if (diagnosticsFailed) {
    return true;
  }
  if (!diagnostics) {
    return false;
  }
  if (diagnostics.mixedAuthModes) {
    return true;
  }
  const hasGithubHttpsRemote =
    isGithubHttpsRemote(diagnostics.fetchUrl) ||
    isGithubHttpsRemote(diagnostics.pushUrl);
  if (
    hasGithubHttpsRemote &&
    diagnostics.githubHttpsCredentialHelperConfigured !== true
  ) {
    return true;
  }
  if (isTransientGitHubConnectionState(ghStatus)) {
    return project.githubPrEnabled || hasGithubHttpsRemote;
  }
  if (project.githubPrEnabled && requiresGitHubCredentialRepair(ghStatus)) {
    return true;
  }
  return requiresGitHubCredentialRepair(ghStatus) && hasGithubHttpsRemote;
}

export function useGitAuthStartupNotification() {
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const project = useProjectStore(selectActiveProject);
  const diagnosticsQuery = useGitAuthDiagnostics(project?.id ?? null);
  const ghStatusQuery = useGitHubConnectionStatus();
  const resumeDeferredGitStartup = useResumeDeferredGitStartup();
  const previouslyBlockedProjects = useRef(new Set<string>());
  const resumeAttemptedProjects = useRef(new Set<string>());

  const hasIssue = !isRemoteEnvironment && hasStartupGitAuthIssue(
    project,
    diagnosticsQuery.data,
    ghStatusQuery.data,
    diagnosticsQuery.isError,
  );

  useEffect(() => {
    if (!project) {
      return;
    }
    if (hasIssue) {
      previouslyBlockedProjects.current.add(project.id);
      resumeAttemptedProjects.current.delete(project.id);
    }
  }, [hasIssue, project]);

  useEffect(() => {
    if (!project) {
      return;
    }
    if (diagnosticsQuery.isLoading || ghStatusQuery.isLoading || hasIssue) {
      return;
    }
    if (!previouslyBlockedProjects.current.has(project.id)) {
      return;
    }
    if (resumeAttemptedProjects.current.has(project.id)) {
      return;
    }

    resumeAttemptedProjects.current.add(project.id);
    resumeDeferredGitStartup.mutate(undefined, {
      onError: () => {
        resumeAttemptedProjects.current.delete(project.id);
      },
    });
  }, [
    diagnosticsQuery.isLoading,
    ghStatusQuery.isLoading,
    hasIssue,
    project,
    resumeDeferredGitStartup,
  ]);
}
