import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createElement, type ReactNode } from "react";

import type { GitHubConnectionStatus } from "@/api/github";
import { githubApi } from "@/api/github";

import { useGitHubConnectionStatus } from "./useGitHubConnectionStatus";

vi.mock("@/api/github", () => ({
  githubApi: {
    getConnectionStatus: vi.fn(),
    getPullRequestDetail: vi.fn(),
  },
}));

const CONNECTED: GitHubConnectionStatus = {
  state: "authenticated",
  diagnostic: null,
  ghInstalled: true,
  authenticated: true,
  host: "github.com",
  account: "octocat",
};

function renderConnectionHook(options?: { enabled?: boolean }) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return {
    queryClient,
    ...renderHook(() => useGitHubConnectionStatus(options ?? {}), { wrapper }),
  };
}

describe("useGitHubConnectionStatus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("queries the live gh connection status", async () => {
    vi.mocked(githubApi.getConnectionStatus).mockResolvedValue(CONNECTED);

    const { result } = renderConnectionHook();

    await waitFor(() => expect(result.current.data).toEqual(CONNECTED));
    expect(githubApi.getConnectionStatus).toHaveBeenCalledTimes(1);
  });

  it("respects enabled:false (does not fetch)", async () => {
    const { result } = renderConnectionHook({ enabled: false });

    await waitFor(() => expect(result.current.fetchStatus).toBe("idle"));
    expect(githubApi.getConnectionStatus).not.toHaveBeenCalled();
  });
});
