import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { automationsApi } from "@/api/automations";

import {
  automationKeys,
  invalidateAutomationQueries,
  useAutomationEvents,
  useCreateAutomationDraft,
} from "./useAutomations";

const subscribeMock = vi.fn();

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: subscribeMock,
  }),
}));

vi.mock("@/api/automations", () => ({
  automationsApi: {
    createDraft: vi.fn(),
  },
}));

function wrapperFor(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("useAutomations", () => {
  beforeEach(() => {
    subscribeMock.mockReset();
  });

  it("invalidates list and detail query scopes", () => {
    const queryClient = new QueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    invalidateAutomationQueries(queryClient, "automation-1");
    invalidateAutomationQueries(queryClient, null);

    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: automationKeys.lists() });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: automationKeys.detail("automation-1"),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: automationKeys.details(),
    });
  });

  it("subscribes to automation events, filters mismatched ids, and unsubscribes", () => {
    const queryClient = new QueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const unsubscribeAutomation = vi.fn();
    const unsubscribeRun = vi.fn();
    const handlers = new Map<string, (payload: unknown) => void>();
    subscribeMock.mockImplementation((eventName: string, handler: (payload: unknown) => void) => {
      handlers.set(eventName, handler);
      return eventName === "automation:updated" ? unsubscribeAutomation : unsubscribeRun;
    });

    const { unmount } = renderHook(() => useAutomationEvents("automation-1"), {
      wrapper: wrapperFor(queryClient),
    });

    expect(subscribeMock).toHaveBeenCalledWith("automation:updated", expect.any(Function));
    expect(subscribeMock).toHaveBeenCalledWith("automation:run:updated", expect.any(Function));

    act(() => {
      handlers.get("automation:updated")?.({ automation_id: "automation-2" });
      handlers.get("automation:run:updated")?.({ automationId: "automation-1" });
      handlers.get("automation:updated")?.({});
    });

    expect(invalidateSpy).not.toHaveBeenCalledWith({
      queryKey: automationKeys.detail("automation-2"),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: automationKeys.detail("automation-1"),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: automationKeys.lists() });

    unmount();

    expect(unsubscribeAutomation).toHaveBeenCalledTimes(1);
    expect(unsubscribeRun).toHaveBeenCalledTimes(1);
  });

  it("useCreateAutomationDraft creates a draft and invalidates list + detail scopes", async () => {
    const queryClient = new QueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const createDraftMock = vi.mocked(automationsApi.createDraft);
    createDraftMock.mockResolvedValue({
      automation: { id: "automation-1" } as never,
      setupConversationId: "conversation-9",
    });

    const { result } = renderHook(() => useCreateAutomationDraft(), {
      wrapper: wrapperFor(queryClient),
    });

    let response: Awaited<ReturnType<typeof result.current.mutateAsync>> | undefined;
    await act(async () => {
      response = await result.current.mutateAsync({ projectId: "project-1" });
    });

    expect(createDraftMock).toHaveBeenCalledWith({ projectId: "project-1" });
    expect(response).toEqual({
      automation: { id: "automation-1" },
      setupConversationId: "conversation-9",
    });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: automationKeys.lists() });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: automationKeys.detail("automation-1"),
    });
  });
});
