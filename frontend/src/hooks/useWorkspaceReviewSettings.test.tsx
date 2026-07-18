import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { workspaceReviewSettingsApi } from "@/api/workspace-review-settings";
import { manualRoleDefaultKeys } from "@/hooks/useManualRoleDefaults";

import {
  useWorkspaceReviewRuntimeSettings,
  workspaceReviewSettingsKeys,
} from "./useWorkspaceReviewSettings";

vi.mock("@/api/workspace-review-settings", () => ({
  workspaceReviewSettingsApi: {
    list: vi.fn(),
    update: vi.fn(),
  },
}));

describe("useWorkspaceReviewRuntimeSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(workspaceReviewSettingsApi.list).mockResolvedValue([]);
    vi.mocked(workspaceReviewSettingsApi.update).mockResolvedValue({
      projectId: null,
      provider: "codex",
      model: "gpt-legacy-review",
      effort: "high",
      updatedAt: "2026-07-18T00:00:00Z",
    });
  });

  it("invalidates legacy settings and the effective manual-role catalog after an update", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(
      () => useWorkspaceReviewRuntimeSettings(null),
      { wrapper },
    );

    act(() => {
      result.current.updateSettings({
        provider: "codex",
        model: "gpt-legacy-review",
        effort: "high",
      });
    });

    await waitFor(() =>
      expect(workspaceReviewSettingsApi.update).toHaveBeenCalledOnce(),
    );
    await waitFor(() => {
      expect(invalidateQueries).toHaveBeenCalledWith({
        queryKey: workspaceReviewSettingsKeys.all,
      });
      expect(invalidateQueries).toHaveBeenCalledWith({
        queryKey: manualRoleDefaultKeys.all,
      });
    });
  });
});
