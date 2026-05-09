import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";

import { mergePipelineApi, type MergePipelineResponse } from "@/api/merge-pipeline";
import { useMergePipeline } from "./useMergePipeline";

vi.mock("@/api/merge-pipeline", () => ({
  mergePipelineApi: {
    getMergePipeline: vi.fn(),
  },
}));

const mockResponse: MergePipelineResponse = {
  active: [],
  waiting: [],
  needsAttention: [],
};

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

describe("useMergePipeline", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("fetches merge pipeline data by default", async () => {
    vi.mocked(mergePipelineApi.getMergePipeline).mockResolvedValue(mockResponse);

    const { result } = renderHook(() => useMergePipeline("project-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.isSuccess).toBe(true);
    });

    expect(mergePipelineApi.getMergePipeline).toHaveBeenCalledWith("project-1");
  });

  it("does not fetch merge pipeline data when disabled", () => {
    vi.mocked(mergePipelineApi.getMergePipeline).mockResolvedValue(mockResponse);

    const { result } = renderHook(
      () => useMergePipeline("project-1", { enabled: false }),
      { wrapper: createWrapper() }
    );

    expect(mergePipelineApi.getMergePipeline).not.toHaveBeenCalled();
    expect(result.current.fetchStatus).toBe("idle");
  });
});
