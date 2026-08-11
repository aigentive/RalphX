import { describe, expect, it } from "vitest";

import { hasStartupGitAuthIssue } from "./useGitAuthStartupNotification";
import type { GitHubConnectionStatus } from "@/api/github";
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
    githubHttpsCredentialHelperConfigured: false,
    canSwitchToSsh: false,
    suggestedSshUrl: null,
    ...overrides,
  };
}

function ghStatus(
  state: GitHubConnectionStatus["state"],
): GitHubConnectionStatus {
  return {
    state,
    diagnostic:
      state === "authenticated"
        ? null
        : state === "provider_unavailable"
          ? "http5xx"
          : "missing_credentials",
    ghInstalled: state !== "cli_unavailable",
    authenticated: state === "authenticated",
    host: state === "cli_unavailable" ? null : "github.com",
    account: state === "authenticated" ? "octocat" : null,
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
        ghStatus("authenticated"),
      ),
    ).toBe(true);
  });

  it("flags GitHub PR mode when gh is not authenticated", () => {
    expect(
      hasStartupGitAuthIssue(project(), diagnostics(), ghStatus("unauthenticated")),
    ).toBe(true);
  });

  it("flags transient provider failures without treating them as credential repair", () => {
    expect(
      hasStartupGitAuthIssue(
        project(),
        diagnostics(),
        ghStatus("provider_unavailable"),
      ),
    ).toBe(true);
  });

  it("does not flag an SSH project without PR mode when gh is missing", () => {
    expect(
      hasStartupGitAuthIssue(
        project({ githubPrEnabled: false }),
        diagnostics(),
        ghStatus("unauthenticated"),
      ),
    ).toBe(false);
  });

  it("flags GitHub HTTPS without a credential helper even when gh is authenticated", () => {
    expect(
      hasStartupGitAuthIssue(
        project({ githubPrEnabled: false }),
        diagnostics({
          fetchUrl: "https://github.com/owner/repo.git",
          pushUrl: "https://github.com/owner/repo.git",
          fetchKind: "HTTPS",
          pushKind: "HTTPS",
          githubHttpsCredentialHelperConfigured: false,
        }),
        ghStatus("authenticated"),
      ),
    ).toBe(true);
  });

  it("does not flag GitHub HTTPS with a credential helper when gh is authenticated", () => {
    expect(
      hasStartupGitAuthIssue(
        project({ githubPrEnabled: false }),
        diagnostics({
          fetchUrl: "https://github.com/owner/repo.git",
          pushUrl: "https://github.com/owner/repo.git",
          fetchKind: "HTTPS",
          pushKind: "HTTPS",
          githubHttpsCredentialHelperConfigured: true,
          canSwitchToSsh: true,
          suggestedSshUrl: "git@github.com:owner/repo.git",
        }),
        ghStatus("authenticated"),
      ),
    ).toBe(false);
  });
});
