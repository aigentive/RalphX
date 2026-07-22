import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { createElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { personaArtifactKeys } from "./personaArtifactQueries";
import { chatKeys } from "./useChat";
import {
  fetchPersonas,
  fetchPersona,
  fetchPersonaOverlayPreview,
  fetchPersonaUsage,
  personaKeys,
  useApprovePersona,
  useArchivePersona,
  useCreatePersonaDraft,
  useDeletePersonaDraft,
  useReseedPersonaDraft,
  useSwitchConversationPersona,
  useUnarchivePersona,
  useUpdatePersona,
  useUpdatePersonaDraft,
} from "./usePersonas";

const personaResponse = {
  id: "persona-1",
  slug: "focused-reviewer",
  name: "Focused Reviewer",
  description: "Reviews changes precisely.",
  content: "---\nname: focused-reviewer\n---",
  status: "draft",
  version: 1,
  project_id: null,
  content_hash: "hash-1",
  source_session_id: null,
  created_at: "2026-07-12T10:00:00Z",
  updated_at: "2026-07-12T10:00:00Z",
};

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

function createQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
}

describe("personaKeys", () => {
  it("creates stable hierarchical keys", () => {
    expect(personaKeys.all).toEqual(["personas"]);
    expect(personaKeys.list()).toEqual(["personas", "list"]);
    expect(personaKeys.detail("persona-1")).toEqual([
      "personas",
      "detail",
      "persona-1",
    ]);
  });
});

describe("persona fetchers", () => {
  it("parses list_personas responses into camelCase Persona values", async () => {
    vi.mocked(invoke).mockResolvedValue([personaResponse]);

    await expect(fetchPersonas()).resolves.toEqual([
      expect.objectContaining({
        contentHash: "hash-1",
        projectId: null,
        sourceSessionId: null,
      }),
    ]);
    expect(invoke).toHaveBeenCalledWith("list_personas", { input: {} });
  });

  it.each([
    ["all", { type: "all" }],
    ["global only", { type: "globalOnly" }],
    [
      "global and project",
      { type: "globalAndProject", projectId: "project-1" },
    ],
  ] as const)("passes the exact %s scope DTO to list_personas", async (_label, scope) => {
    vi.mocked(invoke).mockResolvedValue([personaResponse]);

    await fetchPersonas(scope);

    expect(invoke).toHaveBeenCalledWith("list_personas", {
      input: { scope },
    });
  });

  it("wraps get_persona input and parses its response", async () => {
    vi.mocked(invoke).mockResolvedValue(personaResponse);

    await expect(fetchPersona("persona-1")).resolves.toMatchObject({
      id: "persona-1",
    });
    expect(invoke).toHaveBeenCalledWith("get_persona", {
      input: { id: "persona-1" },
    });
  });

});

describe("persona mutations", () => {
  const mutations = [
    {
      name: "create",
      useHook: useCreatePersonaDraft,
      input: { slug: "focused-reviewer", content: personaResponse.content },
      command: "create_persona_draft",
      args: {
        input: {
          slug: "focused-reviewer",
          content: personaResponse.content,
        },
      },
      response: personaResponse,
    },
    {
      name: "update",
      useHook: useUpdatePersona,
      input: { id: "persona-1", content: "updated content" },
      command: "update_persona",
      args: { input: { id: "persona-1", content: "updated content" } },
      response: personaResponse,
    },
    {
      name: "update draft",
      useHook: useUpdatePersonaDraft,
      input: {
        id: "persona-1",
        content: "updated draft content",
        expectedContentHash: "hash-1",
      },
      command: "update_persona_draft",
      args: {
        input: {
          id: "persona-1",
          content: "updated draft content",
          expectedContentHash: "hash-1",
        },
      },
      response: personaResponse,
    },
    {
      name: "approve",
      useHook: useApprovePersona,
      input: "persona-1",
      command: "approve_persona",
      args: { input: { id: "persona-1" } },
      response: personaResponse,
    },
    {
      name: "archive",
      useHook: useArchivePersona,
      input: "persona-1",
      command: "archive_persona",
      args: { input: { id: "persona-1" } },
      response: personaResponse,
    },
    {
      name: "delete draft",
      useHook: useDeletePersonaDraft,
      input: "persona-1",
      command: "delete_persona_draft",
      args: { input: { id: "persona-1" } },
      response: undefined,
    },
    {
      name: "unarchive",
      useHook: useUnarchivePersona,
      input: "persona-1",
      command: "unarchive_persona",
      args: { input: { id: "persona-1" } },
      response: personaResponse,
    },
    {
      name: "reseed draft",
      useHook: useReseedPersonaDraft,
      input: "persona-1",
      command: "reseed_persona_draft",
      args: { input: { id: "persona-1" } },
      response: personaResponse,
    },
  ] as const;

  for (const mutation of mutations) {
    it(`${mutation.name} uses the exact input wrapper and invalidates the list`, async () => {
      const queryClient = createQueryClient();
      const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
      vi.mocked(invoke).mockResolvedValue(mutation.response);
      const { result } = renderHook(() => mutation.useHook(), {
        wrapper: createWrapper(queryClient),
      });

      await act(async () => {
        await result.current.mutateAsync(mutation.input);
      });

      expect(invoke).toHaveBeenCalledWith(mutation.command, mutation.args);
      await waitFor(() =>
        expect(invalidateQueries).toHaveBeenCalledWith({
          queryKey: personaKeys.list(),
        }),
      );
    });
  }

  it.each([
    [
      "active",
      useUpdatePersona,
      { id: "persona-1", content: "updated content" },
    ],
    [
      "draft",
      useUpdatePersonaDraft,
      {
        id: "persona-1",
        content: "updated draft content",
        expectedContentHash: "hash-1",
      },
    ],
  ] as const)(
    "invalidates the current artifact after a successful %s Persona update",
    async (_kind, useHook, input) => {
      const queryClient = createQueryClient();
      const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
      vi.mocked(invoke).mockResolvedValue({
        ...personaResponse,
        artifact_id: "artifact-1",
      });
      const { result } = renderHook(() => useHook(), {
        wrapper: createWrapper(queryClient),
      });

      await act(async () => {
        await result.current.mutateAsync(input);
      });

      expect(invalidateQueries).toHaveBeenCalledWith({
        queryKey: personaArtifactKeys.detail("artifact-1"),
      });
      expect(queryClient.getQueryData(personaKeys.detail("persona-1"))).toMatchObject({
        id: "persona-1",
        artifactId: "artifact-1",
      });
    },
  );
});

describe("useSwitchConversationPersona", () => {
  it("uses the exact input wrapper and invalidates persona and conversation queries", async () => {
    const queryClient = createQueryClient();
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    vi.mocked(invoke).mockResolvedValue({ conversation: personaResponse });
    const { result } = renderHook(() => useSwitchConversationPersona(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await result.current.mutateAsync({
        conversationId: "conversation-1",
        personaId: null,
      });
    });

    expect(invoke).toHaveBeenCalledWith("switch_agent_conversation_persona", {
      input: { conversationId: "conversation-1", personaId: null },
    });
    await waitFor(() => {
      expect(invalidateQueries).toHaveBeenCalledWith({
        queryKey: personaKeys.list(),
      });
      expect(invalidateQueries).toHaveBeenCalledWith({
        queryKey: chatKeys.conversations(),
      });
      expect(invalidateQueries).toHaveBeenCalledWith({
        queryKey: personaKeys.usage(),
      });
      expect(invalidateQueries).toHaveBeenCalledWith({
        queryKey: personaKeys.overlayPreview("conversation-1"),
      });
    });
  });
});

describe("derived persona reads", () => {
  it("parses camelCase usage rows without any transform", async () => {
    vi.mocked(invoke).mockResolvedValue([
      {
        personaId: "persona-1",
        boundConversationCount: 2,
        lastRunAt: "2026-07-21T09:00:00Z",
      },
      { personaId: "persona-2", boundConversationCount: 0, lastRunAt: null },
    ]);

    const usage = await fetchPersonaUsage();

    expect(invoke).toHaveBeenCalledWith("list_persona_usage");
    expect(usage[1]).toEqual({
      personaId: "persona-2",
      boundConversationCount: 0,
      lastRunAt: null,
    });
  });

  it("fails closed on malformed usage payloads instead of defaulting to zeros", async () => {
    vi.mocked(invoke).mockResolvedValue([{ personaId: "persona-1" }]);
    await expect(fetchPersonaUsage()).rejects.toThrow();
  });

  it("wraps the overlay preview conversation id and parses null and payloads", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(null);
    await expect(fetchPersonaOverlayPreview("conversation-1")).resolves.toBeNull();
    expect(invoke).toHaveBeenCalledWith("preview_persona_overlay", {
      input: { conversationId: "conversation-1" },
    });

    vi.mocked(invoke).mockResolvedValueOnce({
      personaId: "persona-1",
      slug: "focused-reviewer",
      version: 3,
      renderedBlock: "<ralphx_agent_persona>…</ralphx_agent_persona>",
      skippedReason: null,
    });
    await expect(fetchPersonaOverlayPreview("conversation-1")).resolves.toMatchObject({
      slug: "focused-reviewer",
      version: 3,
    });
  });
});
