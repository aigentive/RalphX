import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { personaArtifactKeys } from "@/hooks/personaArtifactQueries";
import { EventProvider } from "@/providers/EventProvider";

import { usePersonaDraftEvents } from "./usePersonaDraftEvents";

describe("usePersonaDraftEvents", () => {
  it("exposes only draft updates owned by the active builder conversation", async () => {
    vi.mocked(invoke).mockResolvedValue({
      id: "draft-1",
      artifact_id: "artifact-1",
      project_id: null,
      slug: "support-voice",
      name: "Support Voice",
      description: "Support voice",
      content: "Direct.",
      status: "draft",
      version: 1,
      content_hash: "hash-1",
      created_at: "2026-07-17T10:00:00Z",
      updated_at: "2026-07-17T10:00:00Z",
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    function Wrapper({ children }: { children: ReactNode }) {
      return (
        <QueryClientProvider client={queryClient}>
          <EventProvider>{children}</EventProvider>
        </QueryClientProvider>
      );
    }
    const { result } = renderHook(
      () => usePersonaDraftEvents("conversation-1"),
      { wrapper: Wrapper },
    );

    await waitFor(() => expect(window.__eventBus).toBeDefined());
    act(() => {
      window.__eventBus?.emit("persona:draft_updated", {
        draft_id: "foreign-draft",
        version: 1,
        content_hash: "foreign-hash",
        artifact_id: null,
        builder_conversation_id: "conversation-2",
      });
    });
    expect(result.current).toBeNull();

    act(() => {
      window.__eventBus?.emit("persona:draft_updated", {
        draft_id: "draft-1",
        version: 1,
        content_hash: "hash-1",
        artifact_id: "artifact-1",
        builder_conversation_id: "conversation-1",
      });
    });
    await waitFor(() => expect(result.current).toBe("draft-1"));
  });

  it("invalidates the persona artifact history from draft_updated artifact_id", async () => {
    vi.mocked(invoke).mockResolvedValue({
      id: "draft-1",
      artifact_id: "artifact-1",
      project_id: null,
      slug: "support-voice",
      name: "Support Voice",
      description: "Support voice",
      content: "Direct.",
      status: "draft",
      version: 4,
      content_hash: "hash-4",
      created_at: "2026-07-17T10:00:00Z",
      updated_at: "2026-07-17T10:00:00Z",
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    function Wrapper({ children }: { children: ReactNode }) {
      return (
        <QueryClientProvider client={queryClient}>
          <EventProvider>{children}</EventProvider>
        </QueryClientProvider>
      );
    }
    renderHook(() => usePersonaDraftEvents("conversation-1"), { wrapper: Wrapper });

    await waitFor(() => expect(window.__eventBus).toBeDefined());
    act(() => {
      window.__eventBus?.emit("persona:draft_updated", {
        draft_id: "draft-1",
        version: 4,
        content_hash: "hash-4",
        artifact_id: "artifact-1",
        builder_conversation_id: "conversation-1",
      });
    });

    await waitFor(() => {
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: personaArtifactKeys.detail("artifact-1"),
      });
    });
  });

  it("logs a failed authoritative draft refresh", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("draft unavailable"));
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    function Wrapper({ children }: { children: ReactNode }) {
      return (
        <QueryClientProvider client={queryClient}>
          <EventProvider>{children}</EventProvider>
        </QueryClientProvider>
      );
    }
    renderHook(() => usePersonaDraftEvents("conversation-1"), { wrapper: Wrapper });

    await waitFor(() => expect(window.__eventBus).toBeDefined());
    act(() => {
      window.__eventBus?.emit("persona:draft_updated", {
        draft_id: "draft-failed",
        version: 2,
        content_hash: "hash-failed",
        artifact_id: null,
        builder_conversation_id: "conversation-1",
      });
    });

    await waitFor(() =>
      expect(consoleError).toHaveBeenCalledWith(
        "Failed to refresh persona draft after persona:draft_updated",
        expect.any(Error),
      ),
    );
    consoleError.mockRestore();
  });
});
