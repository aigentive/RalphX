import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createElement, type ReactNode } from "react";

import type { PullRequestDetail } from "@/api/github";
import { githubApi } from "@/api/github";
import { ticketingKeys } from "@/hooks/useTicketing";

import {
  PR_DETAIL_CACHE_MS,
  appendOptimisticPrComment,
  buildOptimisticPrComment,
  dedupePrIssueComments,
  prKeys,
  usePullRequestDetail,
  type PullRequestDetailSelector,
} from "./usePullRequestDetail";

vi.mock("@/api/github", () => ({
  githubApi: {
    getPullRequestDetail: vi.fn(),
    getConnectionStatus: vi.fn(),
  },
}));

function loadedDetail(overrides: Partial<PullRequestDetail> = {}): PullRequestDetail {
  return {
    state: "loaded",
    origin: "ownedOutbound",
    description: null,
    checks: [],
    reviewSummary: null,
    issueComments: [],
    reviewThread: [],
    rxConversations: [],
    linkedTickets: [],
    sourcesUnavailable: [],
    ...overrides,
  };
}

function renderPrDetailHook(
  selector: PullRequestDetailSelector | null,
  options?: { enabled?: boolean },
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return {
    queryClient,
    ...renderHook(() => usePullRequestDetail(selector, options ?? {}), { wrapper }),
  };
}

describe("usePullRequestDetail", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("does not fire the query when disabled (deferred until after paint)", async () => {
    const { result } = renderPrDetailHook(
      { projectId: "p1", prNumber: 1 },
      { enabled: false },
    );

    await waitFor(() => expect(result.current.fetchStatus).toBe("idle"));
    expect(githubApi.getPullRequestDetail).not.toHaveBeenCalled();
  });

  it("does not fire the query without a resolvable selector", async () => {
    const { result } = renderPrDetailHook(null);

    await waitFor(() => expect(result.current.fetchStatus).toBe("idle"));
    expect(githubApi.getPullRequestDetail).not.toHaveBeenCalled();
  });

  it("caches the full payload including comments with a staleTime", async () => {
    const detail = loadedDetail({
      issueComments: [
        {
          id: "ic1",
          author: "octocat",
          body: "hello",
          url: null,
          createdAt: null,
          updatedAt: null,
          isBot: false,
          isCodecov: false,
          source: "evidence",
        },
      ],
    });
    vi.mocked(githubApi.getPullRequestDetail).mockResolvedValue(detail);

    const selector: PullRequestDetailSelector = { projectId: "p1", prNumber: 42 };
    const { queryClient, result } = renderPrDetailHook(selector);

    await waitFor(() => expect(result.current.data).toEqual(detail));
    expect(githubApi.getPullRequestDetail).toHaveBeenCalledWith(selector);
    // Comments live in the cached payload ("like ticketing").
    expect(
      queryClient.getQueryData<PullRequestDetail>(prKeys.detail(selector))?.issueComments,
    ).toHaveLength(1);
    expect(PR_DETAIL_CACHE_MS).toBeGreaterThan(0);
  });

  it("does not refetch fresh cached detail when a deferred consumer enables", async () => {
    const selector: PullRequestDetailSelector = {
      projectId: "p1",
      prNumber: 42,
    };
    const detail = loadedDetail();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    queryClient.setQueryData(prKeys.detail(selector), detail);
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);
    const { result, rerender } = renderHook(
      ({ enabled }) => usePullRequestDetail(selector, { enabled }),
      { initialProps: { enabled: false }, wrapper },
    );

    expect(result.current.data).toEqual(detail);
    rerender({ enabled: true });

    await waitFor(() => expect(result.current.fetchStatus).toBe("idle"));
    expect(result.current.data).toEqual(detail);
    expect(githubApi.getPullRequestDetail).not.toHaveBeenCalled();
  });
});

describe("prKeys", () => {
  it("uses a namespace separate from ticketingKeys", () => {
    expect(prKeys.all[0]).toBe("github-pr");
    expect(prKeys.all[0]).not.toBe(ticketingKeys.all[0]);
  });

  it("keys a PR detail by project + number + branch", () => {
    expect(prKeys.detail({ projectId: "p1", prNumber: 42 })).toEqual([
      "github-pr",
      "detail",
      "p1",
      42,
      null,
    ]);
    expect(prKeys.detail({ projectId: "p1", branch: "feat/x" })).toEqual([
      "github-pr",
      "detail",
      "p1",
      null,
      "feat/x",
    ]);
  });
});

describe("optimistic PR comment dedupe", () => {
  it("dedupes issue comments by id (last write wins, stable order)", () => {
    const base = buildOptimisticPrComment({ clientId: "abc", body: "first" });
    const updated = { ...base, body: "second" };

    const deduped = dedupePrIssueComments([base, updated]);

    expect(deduped).toHaveLength(1);
    expect(deduped[0]?.body).toBe("second");
    expect(deduped[0]?.id).toBe("optimistic:abc");
  });

  it("appends an optimistic comment without duplicating on repeat", () => {
    const queryClient = new QueryClient();
    const selector: PullRequestDetailSelector = { projectId: "p1", prNumber: 42 };
    queryClient.setQueryData<PullRequestDetail>(prKeys.detail(selector), loadedDetail());

    const comment = buildOptimisticPrComment({ clientId: "abc", body: "hi" });
    appendOptimisticPrComment(queryClient, selector, comment);
    appendOptimisticPrComment(queryClient, selector, comment);

    const cached = queryClient.getQueryData<PullRequestDetail>(prKeys.detail(selector));
    expect(cached?.issueComments).toHaveLength(1);
    expect(cached?.issueComments[0]?.id).toBe("optimistic:abc");
  });
});
