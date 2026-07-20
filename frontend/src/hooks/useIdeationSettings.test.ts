import { createElement } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ideationApi } from "@/api/ideation";
import { defaultIdeationSettings } from "@/types/ideation-config";
import { useIdeationSettings } from "./useIdeationSettings";

vi.mock("@/api/ideation", () => ({
  ideationApi: {
    settings: {
      get: vi.fn(),
      update: vi.fn(),
    },
  },
}));

const settings = { ...defaultIdeationSettings, tasksEnabled: true };

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

describe("useIdeationSettings", () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });
    vi.mocked(ideationApi.settings.get).mockResolvedValue(settings);
  });

  it("keeps Tasks off and refetches after the backend commits OFF with a drain error", async () => {
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    vi.mocked(ideationApi.settings.update).mockRejectedValue(
      new Error("ralphx:tasks_drain_incomplete: retry cleanup"),
    );
    const { result } = renderHook(() => useIdeationSettings(), {
      wrapper: createWrapper(queryClient),
    });

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await act(async () => {
      result.current.updateSettings({ ...settings, tasksEnabled: false });
    });

    await waitFor(() => {
      expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ["ideation", "settings"] });
    });
  });

  it("rolls back an ordinary failed settings update", async () => {
    vi.mocked(ideationApi.settings.update).mockRejectedValue(new Error("network failure"));
    const { result } = renderHook(() => useIdeationSettings(), {
      wrapper: createWrapper(queryClient),
    });

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await act(async () => {
      result.current.updateSettings({ ...settings, tasksEnabled: false });
    });

    await waitFor(() => {
      expect(result.current.settings.tasksEnabled).toBe(true);
      expect(result.current.updateError).toBeInstanceOf(Error);
    });
  });
});
