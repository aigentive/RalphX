/**
 * useProposals hooks tests
 *
 * Tests for useProposals and useProposalMutation hooks
 * using TanStack Query with mocked API.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import {
  useProposals,
  useProposalMutations,
  proposalKeys,
} from "./useProposals";
import { ideationApi } from "@/api/ideation";
import type { TaskProposal as _TaskProposal } from "@/types/ideation";
import type { TaskProposalResponse } from "@/api/ideation";

// Mock the ideation API
vi.mock("@/api/ideation", () => ({
  ideationApi: {
    proposals: {
      list: vi.fn(),
      create: vi.fn(),
      update: vi.fn(),
      // Wave D renamed delete_task_proposal -> archive_task_proposal: the body always
      // archived, so the api surface is `archive`. The mock key must follow or every
      // assertion resolves against undefined.
      archive: vi.fn(),
      reorder: vi.fn(),
    },
  },
}));

// Create mock data
const mockProposal1: TaskProposalResponse = {
  id: "proposal-1",
  sessionId: "session-1",
  title: "Test Proposal 1",
  description: "First proposal",
  category: "feature",
  steps: ["Step 1", "Step 2"],
  acceptanceCriteria: ["AC 1"],
  suggestedPriority: "high",
  priorityScore: 75,
  priorityReason: "Blocks other tasks",
  estimatedComplexity: "moderate",
  userPriority: null,
  userModified: false,
  status: "pending",
  createdTaskId: null,
  sortOrder: 0,
  createdAt: "2026-01-24T10:00:00Z",
  updatedAt: "2026-01-24T10:00:00Z",
};

const mockProposal2: TaskProposalResponse = {
  id: "proposal-2",
  sessionId: "session-1",
  title: "Test Proposal 2",
  description: "Second proposal",
  category: "setup",
  steps: ["Step A"],
  acceptanceCriteria: [],
  suggestedPriority: "medium",
  priorityScore: 50,
  priorityReason: null,
  estimatedComplexity: "simple",
  userPriority: null,
  userModified: false,
  status: "pending",
  createdTaskId: null,
  sortOrder: 1,
  createdAt: "2026-01-24T10:05:00Z",
  updatedAt: "2026-01-24T10:05:00Z",
};

// Test wrapper with QueryClientProvider
function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        gcTime: 0,
      },
    },
  });

  return function Wrapper({ children }: { children: React.ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

describe("proposalKeys", () => {
  it("should generate correct key for all", () => {
    expect(proposalKeys.all).toEqual(["proposals"]);
  });

  it("should generate correct key for lists", () => {
    expect(proposalKeys.lists()).toEqual(["proposals", "list"]);
  });

  it("should generate correct key for list by session", () => {
    expect(proposalKeys.list("session-1")).toEqual([
      "proposals",
      "list",
      "session-1",
    ]);
  });

  it("should generate correct key for details", () => {
    expect(proposalKeys.details()).toEqual(["proposals", "detail"]);
  });

  it("should generate correct key for detail", () => {
    expect(proposalKeys.detail("proposal-1")).toEqual([
      "proposals",
      "detail",
      "proposal-1",
    ]);
  });
});

describe("useProposals", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("should fetch proposals for session successfully", async () => {
    const mockProposals = [mockProposal1, mockProposal2];
    vi.mocked(ideationApi.proposals.list).mockResolvedValueOnce(mockProposals);

    const { result } = renderHook(() => useProposals("session-1"), {
      wrapper: createWrapper(),
    });

    expect(result.current.isLoading).toBe(true);

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toEqual(mockProposals);
    expect(ideationApi.proposals.list).toHaveBeenCalledWith("session-1");
  });

  it("should return empty array for session with no proposals", async () => {
    vi.mocked(ideationApi.proposals.list).mockResolvedValueOnce([]);

    const { result } = renderHook(() => useProposals("session-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toEqual([]);
  });

  it("should handle fetch error", async () => {
    const error = new Error("Failed to fetch proposals");
    vi.mocked(ideationApi.proposals.list).mockRejectedValueOnce(error);

    const { result } = renderHook(() => useProposals("session-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isError).toBe(true));

    expect(result.current.error).toEqual(error);
  });

  it("should not fetch when sessionId is empty", async () => {
    const { result } = renderHook(() => useProposals(""), {
      wrapper: createWrapper(),
    });

    expect(result.current.isFetching).toBe(false);
    expect(ideationApi.proposals.list).not.toHaveBeenCalled();
  });
});

describe("useProposalMutations", () => {
  describe("createProposal", () => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    afterEach(() => {
      vi.resetAllMocks();
    });

    it("should create a proposal successfully", async () => {
      vi.mocked(ideationApi.proposals.create).mockResolvedValueOnce(mockProposal1);

      const { result } = renderHook(() => useProposalMutations(), {
        wrapper: createWrapper(),
      });

      await act(async () => {
        await result.current.createProposal.mutateAsync({
          sessionId: "session-1",
          title: "Test Proposal 1",
          category: "feature",
        });
      });

      expect(ideationApi.proposals.create).toHaveBeenCalledWith({
        sessionId: "session-1",
        title: "Test Proposal 1",
        category: "feature",
      });
    });

    it("should handle creation error", async () => {
      const error = new Error("Failed to create proposal");
      vi.mocked(ideationApi.proposals.create).mockRejectedValueOnce(error);

      const { result } = renderHook(() => useProposalMutations(), {
        wrapper: createWrapper(),
      });

      await expect(
        act(async () => {
          await result.current.createProposal.mutateAsync({
            sessionId: "session-1",
            title: "Test",
            category: "feature",
          });
        })
      ).rejects.toThrow("Failed to create proposal");
    });
  });

  describe("updateProposal", () => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    afterEach(() => {
      vi.resetAllMocks();
    });

    it("should update a proposal successfully", async () => {
      const updatedProposal = { ...mockProposal1, title: "Updated Title" };
      vi.mocked(ideationApi.proposals.update).mockResolvedValueOnce(updatedProposal);

      const { result } = renderHook(() => useProposalMutations(), {
        wrapper: createWrapper(),
      });

      await act(async () => {
        await result.current.updateProposal.mutateAsync({
          proposalId: "proposal-1",
          changes: { title: "Updated Title" },
        });
      });

      expect(ideationApi.proposals.update).toHaveBeenCalledWith("proposal-1", {
        title: "Updated Title",
      });
    });

    it("should handle update error", async () => {
      const error = new Error("Failed to update proposal");
      vi.mocked(ideationApi.proposals.update).mockRejectedValueOnce(error);

      const { result } = renderHook(() => useProposalMutations(), {
        wrapper: createWrapper(),
      });

      await expect(
        act(async () => {
          await result.current.updateProposal.mutateAsync({
            proposalId: "proposal-1",
            changes: { title: "Updated" },
          });
        })
      ).rejects.toThrow("Failed to update proposal");
    });
  });

  describe("deleteProposal", () => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    afterEach(() => {
      vi.resetAllMocks();
    });

    it("should delete a proposal successfully", async () => {
      vi.mocked(ideationApi.proposals.archive).mockResolvedValueOnce(undefined);

      const { result } = renderHook(() => useProposalMutations(), {
        wrapper: createWrapper(),
      });

      await act(async () => {
        await result.current.deleteProposal.mutateAsync("proposal-1");
      });

      expect(ideationApi.proposals.archive).toHaveBeenCalledWith("proposal-1");
    });

    it("should handle delete error", async () => {
      const error = new Error("Failed to delete proposal");
      vi.mocked(ideationApi.proposals.archive).mockRejectedValueOnce(error);

      const { result } = renderHook(() => useProposalMutations(), {
        wrapper: createWrapper(),
      });

      await expect(
        act(async () => {
          await result.current.deleteProposal.mutateAsync("proposal-1");
        })
      ).rejects.toThrow("Failed to delete proposal");
    });
  });

  describe("reorder", () => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    afterEach(() => {
      vi.resetAllMocks();
    });

    it("should reorder proposals successfully", async () => {
      vi.mocked(ideationApi.proposals.reorder).mockResolvedValueOnce(undefined);

      const { result } = renderHook(() => useProposalMutations(), {
        wrapper: createWrapper(),
      });

      await act(async () => {
        await result.current.reorder.mutateAsync({
          sessionId: "session-1",
          proposalIds: ["proposal-2", "proposal-1"],
        });
      });

      expect(ideationApi.proposals.reorder).toHaveBeenCalledWith("session-1", [
        "proposal-2",
        "proposal-1",
      ]);
    });

    it("should handle reorder error", async () => {
      const error = new Error("Failed to reorder proposals");
      vi.mocked(ideationApi.proposals.reorder).mockRejectedValueOnce(error);

      const { result } = renderHook(() => useProposalMutations(), {
        wrapper: createWrapper(),
      });

      await expect(
        act(async () => {
          await result.current.reorder.mutateAsync({
            sessionId: "session-1",
            proposalIds: ["proposal-2", "proposal-1"],
          });
        })
      ).rejects.toThrow("Failed to reorder proposals");
    });
  });
});
