import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { act } from "react";

import type { GitHubConnectionStatus } from "@/api/github";
import type { Project } from "@/types/project";

// ---------------------------------------------------------------------------
// Mocks (declared before importing the hook)
// ---------------------------------------------------------------------------

const {
  toastFn,
  mockResumeMutate,
  mocks,
} = vi.hoisted(() => {
  const mocks = {
    activeProject: null as Project | null,
    diagnostics: { data: undefined, isLoading: false, isError: false } as {
      data?: unknown;
      isLoading?: boolean;
      isError?: boolean;
    },
    ghAuth: { data: undefined, isLoading: false } as {
      data?: unknown;
      isLoading?: boolean;
    },
  };
  return {
    toastFn: vi.fn(),
    mockResumeMutate: vi.fn(),
    mocks,
  };
});

vi.mock("sonner", () => ({
  toast: toastFn,
}));

vi.mock("@/stores/projectStore", () => ({
  selectActiveProject: () => mocks.activeProject,
  useProjectStore: (selector: (s: unknown) => unknown) => selector(undefined),
}));

vi.mock("@/hooks/useGithubSettings", () => ({
  useGitAuthDiagnostics: () => mocks.diagnostics,
  useResumeDeferredGitStartup: () => ({ mutate: mockResumeMutate }),
}));

vi.mock("@/hooks/useGitHubConnectionStatus", () => ({
  useGitHubConnectionStatus: () => mocks.ghAuth,
}));

import { useGitAuthStartupNotification } from "./useGitAuthStartupNotification";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeProject(overrides: Partial<Project> = {}): Project {
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

function diagnosticsHttps() {
  return {
    fetchUrl: "https://github.com/owner/repo.git",
    pushUrl: "https://github.com/owner/repo.git",
    fetchKind: "HTTPS",
    pushKind: "HTTPS",
    mixedAuthModes: false,
    githubHttpsCredentialHelperConfigured: false,
    canSwitchToSsh: true,
    suggestedSshUrl: "git@github.com:owner/repo.git",
  };
}

function diagnosticsSsh() {
  return {
    fetchUrl: "git@github.com:owner/repo.git",
    pushUrl: "git@github.com:owner/repo.git",
    fetchKind: "SSH",
    pushKind: "SSH",
    mixedAuthModes: false,
    githubHttpsCredentialHelperConfigured: false,
    canSwitchToSsh: false,
    suggestedSshUrl: null,
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

// ---------------------------------------------------------------------------

describe("useGitAuthStartupNotification (hook)", () => {
  beforeEach(() => {
    toastFn.mockClear();
    mockResumeMutate.mockClear();
    mocks.activeProject = null;
    mocks.diagnostics = { data: undefined, isLoading: false, isError: false };
    mocks.ghAuth = { data: undefined, isLoading: false };
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("does nothing when there is no active project", () => {
    renderHook(() => useGitAuthStartupNotification());
    expect(toastFn).not.toHaveBeenCalled();
  });

  it("leaves the startup Git auth alert to its durable notification-center row", () => {
    mocks.activeProject = makeProject();
    mocks.diagnostics = { data: diagnosticsHttps(), isLoading: false, isError: false };
    mocks.ghAuth = { data: ghStatus("unauthenticated"), isLoading: false };

    renderHook(() => useGitAuthStartupNotification());
    expect(toastFn).not.toHaveBeenCalled();
  });

  it("does not add a bespoke toast when the durable startup condition re-renders", () => {
    mocks.activeProject = makeProject();
    mocks.diagnostics = { data: diagnosticsHttps(), isLoading: false, isError: false };
    mocks.ghAuth = { data: ghStatus("unauthenticated"), isLoading: false };

    const { rerender } = renderHook(() => useGitAuthStartupNotification());
    rerender();
    rerender();
    expect(toastFn).not.toHaveBeenCalled();
  });

  it("resumes deferred startup when a previously blocked project becomes healthy", () => {
    mocks.activeProject = makeProject();
    mocks.diagnostics = { data: diagnosticsHttps(), isLoading: false, isError: false };
    mocks.ghAuth = { data: ghStatus("unauthenticated"), isLoading: false };

    const { rerender } = renderHook(() => useGitAuthStartupNotification());
    expect(toastFn).not.toHaveBeenCalled();

    // Project recovers — gh now authenticated, fetch/push back on SSH.
    act(() => {
      mocks.diagnostics = { data: diagnosticsSsh(), isLoading: false, isError: false };
      mocks.ghAuth = { data: ghStatus("authenticated"), isLoading: false };
    });
    rerender();
    expect(mockResumeMutate).toHaveBeenCalledTimes(1);
  });

  it("does not toast while diagnostics load", () => {
    mocks.activeProject = makeProject();
    mocks.diagnostics = { data: undefined, isLoading: true, isError: false };
    mocks.ghAuth = { data: undefined, isLoading: true };
    renderHook(() => useGitAuthStartupNotification());
    expect(toastFn).not.toHaveBeenCalled();
  });

  it("does not add a bespoke toast when diagnostics fail", () => {
    mocks.activeProject = makeProject();
    mocks.diagnostics = { data: undefined, isLoading: false, isError: true };
    mocks.ghAuth = { data: ghStatus("provider_unavailable"), isLoading: false };

    renderHook(() => useGitAuthStartupNotification());
    expect(toastFn).not.toHaveBeenCalled();
  });
});
