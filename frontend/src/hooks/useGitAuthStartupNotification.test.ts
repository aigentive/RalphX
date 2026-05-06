import { describe, expect, it } from "vitest";

import {
  GIT_AUTH_STARTUP_TOAST_DURATION,
  createStartupGitAuthToastOptions,
  hasStartupGitAuthIssue,
} from "./useGitAuthStartupNotification";
import type { GitAuthDiagnostics } from "./useGithubSettings";
import type { Project } from "@/types/project";

function project(overrides: Partial<Project> = {}): Project {
  return {
    id: "project-1",
    name: "RalphX",
    workingDirectory: "/repo",
    gitMode: "worktree",
    baseBranch: "main",
    worktreeParentDirectory: null,
    useFeatureBranches: true,
    mergeValidationMode: "block",
    detectedAnalysis: null,
    customAnalysis: null,
    analyzedAt: null,
    githubPrEnabled: true,
    createdAt: "2026-05-01T00:00:00Z",
    updatedAt: "2026-05-01T00:00:00Z",
    ...overrides,
  };
}

function diagnostics(overrides: Partial<GitAuthDiagnostics> = {}): GitAuthDiagnostics {
  return {
    fetchUrl: "git@github.com:owner/repo.git",
    pushUrl: "git@github.com:owner/repo.git",
    fetchKind: "SSH",
    pushKind: "SSH",
    mixedAuthModes: false,
    canSwitchToSsh: false,
    suggestedSshUrl: null,
    ...overrides,
  };
}

describe("hasStartupGitAuthIssue", () => {
  it("flags mixed fetch and push auth modes", () => {
    expect(
      hasStartupGitAuthIssue(
        project(),
        diagnostics({
          fetchUrl: "https://github.com/owner/repo.git",
          fetchKind: "HTTPS",
          mixedAuthModes: true,
          canSwitchToSsh: true,
          suggestedSshUrl: "git@github.com:owner/repo.git",
        }),
        true,
      ),
    ).toBe(true);
  });

  it("flags GitHub PR mode when gh is not authenticated", () => {
    expect(hasStartupGitAuthIssue(project(), diagnostics(), false)).toBe(true);
  });

  it("does not flag an SSH project without PR mode when gh is missing", () => {
    expect(
      hasStartupGitAuthIssue(
        project({ githubPrEnabled: false }),
        diagnostics(),
        false,
      ),
    ).toBe(false);
  });
});

describe("createStartupGitAuthToastOptions", () => {
  it("keeps git auth startup warnings visible until the user acts", () => {
    const options = createStartupGitAuthToastOptions("project-1");

    expect(options.duration).toBe(GIT_AUTH_STARTUP_TOAST_DURATION);
    expect(options.duration).toBe(Infinity);
    expect(options.id).toBe("git-auth-startup:project-1");
    expect(options.className).toBe("git-auth-startup-toast");
  });
});
