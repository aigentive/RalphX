import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { updateChannelApi } from "@/api/update-channel";

import { updateChannelKeys, useUpdateChannel } from "./useUpdateChannel";

vi.mock("@/api/update-channel", () => ({
  updateChannelApi: {
    get: vi.fn(),
    set: vi.fn(),
  },
}));

function createWrapper(queryClient: QueryClient) {
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

describe("useUpdateChannel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(updateChannelApi.get).mockResolvedValue("stable");
    vi.mocked(updateChannelApi.set).mockResolvedValue("nightly");
  });

  it("does not treat the stable fallback as settled before persistence loads", async () => {
    let resolveChannel: ((channel: "stable" | "nightly") => void) | undefined;
    vi.mocked(updateChannelApi.get).mockImplementation(
      () => new Promise((resolve) => {
        resolveChannel = resolve;
      }),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { result } = renderHook(() => useUpdateChannel(), {
      wrapper: createWrapper(queryClient),
    });

    expect(result.current.updateChannel).toBe("stable");
    expect(result.current.isSettled).toBe(false);

    act(() => resolveChannel?.("nightly"));

    await waitFor(() => expect(result.current.isSettled).toBe(true));
    expect(result.current.updateChannel).toBe("nightly");
  });

  it("writes then invalidates the shared channel query after a successful save", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    vi.mocked(updateChannelApi.get)
      .mockResolvedValueOnce("stable")
      .mockResolvedValueOnce("nightly");
    const { result } = renderHook(() => useUpdateChannel(), {
      wrapper: createWrapper(queryClient),
    });

    await waitFor(() => expect(result.current.isSettled).toBe(true));

    act(() => result.current.setUpdateChannel("nightly"));

    await waitFor(() => expect(updateChannelApi.set).toHaveBeenCalledWith("nightly"));
    await waitFor(() =>
      expect(queryClient.getQueryData(updateChannelKeys.current())).toBe("nightly"),
    );
    expect(result.current.updateChannel).toBe("nightly");
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: updateChannelKeys.current(),
    });
  });

  it("exposes mutation pending state while saving the selected channel", async () => {
    let resolveSave: ((channel: "stable" | "nightly") => void) | undefined;
    vi.mocked(updateChannelApi.set).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveSave = resolve;
        }),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    const { result } = renderHook(() => useUpdateChannel(), {
      wrapper: createWrapper(queryClient),
    });

    await waitFor(() => expect(result.current.isSettled).toBe(true));
    act(() => result.current.setUpdateChannel("nightly"));

    await waitFor(() => expect(result.current.isSaving).toBe(true));
    expect(result.current.saveError).toBeNull();

    act(() => resolveSave?.("nightly"));
    await waitFor(() => expect(result.current.isSaving).toBe(false));
  });

  it("exposes a failed save without pretending Nightly persisted", async () => {
    const saveError = new Error("write failed");
    vi.mocked(updateChannelApi.set).mockRejectedValue(saveError);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    const { result } = renderHook(() => useUpdateChannel(), {
      wrapper: createWrapper(queryClient),
    });

    await waitFor(() => expect(result.current.isSettled).toBe(true));
    act(() => result.current.setUpdateChannel("nightly"));

    await waitFor(() => expect(result.current.saveError).toBe(saveError));
    expect(result.current.isSaving).toBe(false);
    expect(result.current.updateChannel).toBe("stable");
    expect(queryClient.getQueryData(updateChannelKeys.current())).toBe("stable");
  });
});
