/**
 * useGithubSettings tests — verify Tauri invoke is called with correct args.
 *
 * Tests query/mutation hooks via direct invocation of the underlying functions.
 * The global Tauri mock is set up in src/test/setup.ts.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import type { ReactNode } from "react";
import { prKeys } from "./usePullRequestDetail";
import {
  useGitRemoteUrl,
  useGitAuthDiagnostics,
  useGhAuthStatus,
  useLoginGhWithBrowser,
  useSwitchGitOriginToSsh,
  useSetupGhGitAuth,
  useResumeDeferredGitStartup,
  useUpdateGithubPrEnabled,
} from "./useGithubSettings";

// ============================================================================
// Test helpers
// ============================================================================

function createWrapper(queryClient?: QueryClient) {
  const client = queryClient ?? new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
}

// ============================================================================
// useGitRemoteUrl
// ============================================================================

describe("useGitRemoteUrl", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("is disabled (does not call invoke) when projectId is null", async () => {
    const { result } = renderHook(() => useGitRemoteUrl(null), {
      wrapper: createWrapper(),
    });

    // Query is disabled — data remains undefined, no invoke call
    expect(result.current.data).toBeUndefined();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("calls get_git_remote_url with projectId when enabled", async () => {
    vi.mocked(invoke).mockResolvedValue("https://github.com/org/repo.git");

    const { result } = renderHook(() => useGitRemoteUrl("proj-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(invoke).toHaveBeenCalledWith("get_git_remote_url", {
      projectId: "proj-1",
    });
    expect(result.current.data).toBe("https://github.com/org/repo.git");
  });

  it("returns null when backend returns null (no remote configured)", async () => {
    vi.mocked(invoke).mockResolvedValue(null);

    const { result } = renderHook(() => useGitRemoteUrl("proj-2"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toBeNull();
  });

  it("exposes isLoading and isError states", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("network error"));

    const { result } = renderHook(() => useGitRemoteUrl("proj-3"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isError).toBe(true));

    expect(result.current.isLoading).toBe(false);
    expect(result.current.error).toBeDefined();
  });
});

// ============================================================================
// useGitAuthDiagnostics
// ============================================================================

describe("useGitAuthDiagnostics", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("is disabled when projectId is null", () => {
    const { result } = renderHook(() => useGitAuthDiagnostics(null), {
      wrapper: createWrapper(),
    });

    expect(result.current.data).toBeUndefined();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("calls get_git_auth_diagnostics with projectId when enabled", async () => {
    const diagnostics = {
      fetchUrl: "https://github.com/org/repo.git",
      pushUrl: "git@github.com:org/repo.git",
      fetchKind: "HTTPS",
      pushKind: "SSH",
      mixedAuthModes: true,
      githubHttpsCredentialHelperConfigured: false,
      canSwitchToSsh: true,
      suggestedSshUrl: "git@github.com:org/repo.git",
    };
    vi.mocked(invoke).mockResolvedValue(diagnostics);

    const { result } = renderHook(() => useGitAuthDiagnostics("proj-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(invoke).toHaveBeenCalledWith("get_git_auth_diagnostics", {
      projectId: "proj-1",
    });
    expect(result.current.data).toEqual(diagnostics);
  });
});

// ============================================================================
// useGhAuthStatus
// ============================================================================

describe("useGhAuthStatus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("derives true from canonical connection status when authenticated", async () => {
    vi.mocked(invoke).mockResolvedValue({
      state: "authenticated",
      diagnostic: null,
      ghInstalled: true,
      authenticated: true,
      host: "github.com",
      account: "octocat",
    });

    const { result } = renderHook(() => useGhAuthStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(invoke).toHaveBeenCalledWith("get_github_connection_status", {});
    expect(result.current.data).toBe(true);
  });

  it("returns false when canonical connection status is not authenticated", async () => {
    vi.mocked(invoke).mockResolvedValue({
      state: "provider_unavailable",
      diagnostic: "http5xx",
      ghInstalled: true,
      authenticated: false,
      host: "github.com",
      account: null,
    });

    const { result } = renderHook(() => useGhAuthStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toBe(false);
  });

  it("exposes isLoading and isError states", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("gh not installed"));

    const { result } = renderHook(() => useGhAuthStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isError).toBe(true));

    expect(result.current.isLoading).toBe(false);
  });
});

// ============================================================================
// Git auth repair mutations
// ============================================================================

describe("git auth repair mutations", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("calls switch_git_origin_to_ssh with projectId", async () => {
    vi.mocked(invoke).mockResolvedValue({
      fetchUrl: "git@github.com:org/repo.git",
      pushUrl: "git@github.com:org/repo.git",
      fetchKind: "SSH",
      pushKind: "SSH",
      mixedAuthModes: false,
      githubHttpsCredentialHelperConfigured: false,
      canSwitchToSsh: false,
      suggestedSshUrl: null,
    });

    const { result } = renderHook(() => useSwitchGitOriginToSsh(), {
      wrapper: createWrapper(),
    });

    result.current.mutate({ projectId: "proj-1" });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(invoke).toHaveBeenCalledWith("switch_git_origin_to_ssh", {
      projectId: "proj-1",
    });
  });

  it("calls setup_gh_git_auth without project args", async () => {
    vi.mocked(invoke).mockResolvedValue(true);
    const queryClient = new QueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

    const { result } = renderHook(() => useSetupGhGitAuth(), {
      wrapper: createWrapper(queryClient),
    });

    result.current.mutate();

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(invoke).toHaveBeenCalledWith("setup_gh_git_auth", {});
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["gh-auth-status"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: prKeys.connectionStatus(),
    });
  });

  it("calls login_gh_with_browser without project args", async () => {
    vi.mocked(invoke).mockResolvedValue(true);
    const queryClient = new QueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

    const { result } = renderHook(() => useLoginGhWithBrowser(), {
      wrapper: createWrapper(queryClient),
    });

    result.current.mutate();

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(invoke).toHaveBeenCalledWith("login_gh_with_browser", {});
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["gh-auth-status"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: prKeys.connectionStatus(),
    });
  });

  it("calls resume_deferred_git_startup without project args", async () => {
    vi.mocked(invoke).mockResolvedValue(true);

    const { result } = renderHook(() => useResumeDeferredGitStartup(), {
      wrapper: createWrapper(),
    });

    result.current.mutate();

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(invoke).toHaveBeenCalledWith("resume_deferred_git_startup", {});
  });
});

// ============================================================================
// useUpdateGithubPrEnabled
// ============================================================================

describe("useUpdateGithubPrEnabled", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("calls update_github_pr_enabled with projectId and enabled", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);

    const { result } = renderHook(() => useUpdateGithubPrEnabled(), {
      wrapper: createWrapper(),
    });

    result.current.mutate({ projectId: "proj-1", enabled: true });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(invoke).toHaveBeenCalledWith("update_github_pr_enabled", {
      projectId: "proj-1",
      enabled: true,
    });
  });

  it("calls update_github_pr_enabled with enabled=false to disable PR mode", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);

    const { result } = renderHook(() => useUpdateGithubPrEnabled(), {
      wrapper: createWrapper(),
    });

    result.current.mutate({ projectId: "proj-2", enabled: false });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(invoke).toHaveBeenCalledWith("update_github_pr_enabled", {
      projectId: "proj-2",
      enabled: false,
    });
  });

  it("resolves successfully on mutation success", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);

    const { result } = renderHook(() => useUpdateGithubPrEnabled(), {
      wrapper: createWrapper(),
    });

    result.current.mutate({ projectId: "proj-1", enabled: true });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.isError).toBe(false);
  });
});
