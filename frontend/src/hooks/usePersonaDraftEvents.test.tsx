import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import { personaArtifactKeys } from "@/hooks/usePersonaArtifact";
import { EventProvider } from "@/providers/EventProvider";

import { usePersonaDraftEvents } from "./usePersonaDraftEvents";

describe("usePersonaDraftEvents", () => {
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
    renderHook(() => usePersonaDraftEvents(), { wrapper: Wrapper });

    await waitFor(() => expect(window.__eventBus).toBeDefined());
    act(() => {
      window.__eventBus?.emit("persona:draft_updated", {
        draft_id: "draft-1",
        version: 4,
        content_hash: "hash-4",
        artifact_id: "artifact-1",
      });
    });

    await waitFor(() => {
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: personaArtifactKeys.history("artifact-1"),
      });
    });
  });
});
