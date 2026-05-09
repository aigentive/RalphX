import { createElement, useEffect, useMemo, useRef } from "react";
import { AlertTriangle } from "lucide-react";
import { toast } from "sonner";

import {
  useGhAuthStatus,
  useGitAuthDiagnostics,
  useResumeDeferredGitStartup,
} from "@/hooks/useGithubSettings";
import { selectActiveProject, useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";
import type { GitAuthDiagnostics } from "@/hooks/useGithubSettings";
import type { Project } from "@/types/project";

export const GIT_AUTH_STARTUP_TOAST_DURATION = Infinity;

function isGithubHttpsRemote(url: string | null | undefined) {
  return url?.trim().startsWith("https://github.com/") ?? false;
}

export function createStartupGitAuthToastOptions(projectId: string) {
  return {
    duration: GIT_AUTH_STARTUP_TOAST_DURATION,
    id: `git-auth-startup:${projectId}`,
    className: "git-auth-startup-toast",
  };
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
  if (project.githubPrEnabled && ghAuthenticated === false) {
    return true;
  }
  return (
    ghAuthenticated === false &&
    (isGithubHttpsRemote(diagnostics.fetchUrl) || isGithubHttpsRemote(diagnostics.pushUrl))
  );
}

export function useGitAuthStartupNotification() {
  const project = useProjectStore(selectActiveProject);
  const openModal = useUiStore((state) => state.openModal);
  const diagnosticsQuery = useGitAuthDiagnostics(project?.id ?? null);
  const ghAuthQuery = useGhAuthStatus();
  const resumeDeferredGitStartup = useResumeDeferredGitStartup();
  const notifiedKeys = useRef(new Set<string>());
  const previouslyBlockedProjects = useRef(new Set<string>());
  const resumeAttemptedProjects = useRef(new Set<string>());

  const hasIssue = hasStartupGitAuthIssue(
    project,
    diagnosticsQuery.data,
    ghAuthQuery.data,
    diagnosticsQuery.isError,
  );

  const notificationKey = useMemo(() => {
    if (!project || !hasIssue) {
      return null;
    }
    const diagnostics = diagnosticsQuery.data;
    return [
      project.id,
      diagnostics?.fetchKind ?? "unknown-fetch",
      diagnostics?.pushKind ?? "unknown-push",
      diagnostics?.mixedAuthModes ? "mixed" : "same",
      ghAuthQuery.data === false ? "gh-missing" : "gh-ok",
      diagnosticsQuery.isError ? "diagnostics-error" : "diagnostics-ok",
    ].join(":");
  }, [diagnosticsQuery.data, diagnosticsQuery.isError, ghAuthQuery.data, hasIssue, project]);

  useEffect(() => {
    if (!project || !notificationKey) {
      return;
    }
    if (diagnosticsQuery.isLoading || ghAuthQuery.isLoading) {
      return;
    }
    if (notifiedKeys.current.has(notificationKey)) {
      return;
    }

    notifiedKeys.current.add(notificationKey);
    toast(
      createElement(
        "div",
        {
          "data-testid": "git-auth-startup-toast",
          className: "flex flex-col gap-2",
        },
        createElement(
          "div",
          { className: "flex items-center gap-2" },
          createElement(AlertTriangle, {
            className: "h-4 w-4",
            style: { color: "var(--status-warning)" },
          }),
          createElement(
            "span",
            { className: "font-medium" },
            "Repository access needs attention",
          ),
        ),
        createElement(
          "p",
          {
            "data-testid": "git-auth-startup-toast-description",
            className: "text-xs",
            style: {
              color: "var(--text-muted)",
              lineHeight: 1.4,
            },
          },
          `Startup Git/GitHub recovery was paused for ${project.name}. Repair repository access before starting agents or publishing work.`,
        ),
        createElement(
          "div",
          { className: "flex gap-2 mt-1" },
          createElement(
            "button",
            {
              type: "button",
              "data-testid": "git-auth-startup-toast-action",
              className:
                "git-auth-startup-toast-action inline-flex h-7 items-center rounded-[6px] px-3 text-xs font-semibold",
              onClick: () => {
                openModal("settings", { section: "repository" });
                toast.dismiss(`git-auth-startup:${project.id}`);
              },
            },
            "Open Settings",
          ),
          createElement(
            "button",
            {
              type: "button",
              className:
                "inline-flex h-7 items-center rounded-[6px] px-3 text-xs font-medium",
              style: { color: "var(--text-muted)" },
              onClick: () => toast.dismiss(`git-auth-startup:${project.id}`),
            },
            "Later",
          ),
        ),
      ),
      createStartupGitAuthToastOptions(project.id),
    );
  }, [
    diagnosticsQuery.isLoading,
    ghAuthQuery.isLoading,
    notificationKey,
    openModal,
    project,
  ]);

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
