import { useEffect, useRef } from "react";

import {
  useGhAuthStatus,
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
  ghAuthenticated: boolean | undefined,
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
  if (project.githubPrEnabled && ghAuthenticated === false) {
    return true;
  }
  return ghAuthenticated === false && hasGithubHttpsRemote;
}

export function useGitAuthStartupNotification() {
  const project = useProjectStore(selectActiveProject);
  const diagnosticsQuery = useGitAuthDiagnostics(project?.id ?? null);
  const ghAuthQuery = useGhAuthStatus();
  const resumeDeferredGitStartup = useResumeDeferredGitStartup();
  const previouslyBlockedProjects = useRef(new Set<string>());
  const resumeAttemptedProjects = useRef(new Set<string>());

  const hasIssue = hasStartupGitAuthIssue(
    project,
    diagnosticsQuery.data,
    ghAuthQuery.data,
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
    if (diagnosticsQuery.isLoading || ghAuthQuery.isLoading || hasIssue) {
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
    ghAuthQuery.isLoading,
    hasIssue,
    project,
    resumeDeferredGitStartup,
  ]);
}
