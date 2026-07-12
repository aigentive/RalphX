import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { createElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { chatKeys } from "./useChat";
import {
  fetchPersonas,
  fetchPersona,
  ingestPersonaContext,
  personaKeys,
  useApprovePersona,
  useArchivePersona,
  useCreatePersonaDraft,
  useDeletePersonaDraft,
  useSwitchConversationPersona,
  useUpdatePersona,
} from "./usePersonas";

const personaResponse = {
  id: "persona-1",
  slug: "focused-reviewer",
  name: "Focused Reviewer",
  description: "Reviews changes precisely.",
  content: "---\nname: focused-reviewer\n---",
  status: "draft",
  version: 1,
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
    expect(personaKeys.ingestManifest("conversation-1")).toEqual([
      "personas",
      "ingest-manifest",
      "conversation-1",
    ]);
  });
});

describe("persona fetchers", () => {
  it("parses list_personas responses into camelCase Persona values", async () => {
    vi.mocked(invoke).mockResolvedValue([personaResponse]);

    await expect(fetchPersonas()).resolves.toEqual([
      expect.objectContaining({
        contentHash: "hash-1",
        sourceSessionId: null,
      }),
    ]);
    expect(invoke).toHaveBeenCalledWith("list_personas", { input: {} });
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

  it("uses the single picked-path ingest contract", async () => {
    vi.mocked(invoke).mockResolvedValue({
      copied: [{ path: "notes.md" }],
      skipped: [],
      rejected: [],
    });

    await expect(
      ingestPersonaContext({
        conversationId: "conversation-1",
        pickedPath: "/picked/context",
      }),
    ).resolves.toHaveProperty("copied.0.path", "notes.md");
    expect(invoke).toHaveBeenCalledWith("ingest_persona_context", {
      input: {
        conversationId: "conversation-1",
        pickedPath: "/picked/context",
      },
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
    });
  });
});
