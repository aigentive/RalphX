import { beforeEach, describe, expect, it, vi } from "vitest";

import { typedInvoke } from "@/lib/tauri";

import { GranolaIntegrationSettingsSchema, granolaApi } from "./granola";

vi.mock("@/lib/tauri", () => ({
  typedInvoke: vi.fn(),
}));

describe("granolaApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads, saves, validates, and disconnects Granola integration settings", async () => {
    const settings = {
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      lastValidatedAt: "2026-06-17T12:00:00Z",
      lastError: null,
      updatedAt: "2026-06-17T12:00:00Z",
    };
    vi.mocked(typedInvoke).mockResolvedValue(settings);

    await expect(granolaApi.getSettings()).resolves.toEqual(settings);
    await expect(
      granolaApi.saveSettings({ apiToken: "granola-token" }),
    ).resolves.toEqual(settings);
    await expect(granolaApi.validate()).resolves.toEqual(settings);
    await expect(granolaApi.disconnect()).resolves.toEqual(settings);

    expect(typedInvoke).toHaveBeenNthCalledWith(
      1,
      "get_granola_integration_settings",
      {},
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      2,
      "save_granola_integration_settings",
      { input: { apiToken: "granola-token" } },
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      3,
      "validate_granola_integration_settings",
      {},
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      4,
      "save_granola_integration_settings",
      { input: { apiToken: "" } },
      expect.any(Object),
    );
  });

  it("lists note summaries, fetches detail, and manages conversation binding", async () => {
    const listResponse = {
      notes: [
        {
          id: "not_1234567890ABCD",
          title: "Planning sync",
          url: "https://granola.ai/notes/not_1234567890ABCD",
          summary: "Decisions",
          createdAt: "2026-06-20T12:00:00Z",
          updatedAt: "2026-06-20T13:00:00Z",
        },
      ],
      hasMore: true,
      cursor: "next",
    };
    const detail = {
      id: "not_1234567890ABCD",
      title: "Planning sync",
      url: "https://granola.ai/notes/not_1234567890ABCD",
      summary: "Decisions",
      transcript: [{ speaker: "Alex", text: "Ship it", startMs: 10, endMs: 20 }],
    };
    const bound = {
      conversationId: "conversation-1",
      projectId: "project-1",
      provider: "granola",
      noteId: "not_1234567890ABCD",
      noteUrl: "https://granola.ai/notes/not_1234567890ABCD",
      title: "Planning sync",
      summaryMarkdown: "Decisions",
      transcript: [],
      includeTranscript: true,
      lastRefreshedAt: "2026-06-20T13:00:00Z",
      refreshStatus: "loaded",
      refreshError: null,
      assignedAt: "2026-06-20T12:00:00Z",
      assignedFromMessageId: null,
      manuallyAssigned: true,
      createdAt: "2026-06-20T12:00:00Z",
      updatedAt: "2026-06-20T13:00:00Z",
    };
    vi.mocked(typedInvoke)
      .mockResolvedValueOnce(listResponse)
      .mockResolvedValueOnce(detail)
      .mockResolvedValueOnce({ note: null })
      .mockResolvedValueOnce({ note: bound })
      .mockResolvedValueOnce({ note: bound })
      .mockResolvedValueOnce({ note: null });

    await expect(granolaApi.listNotes({ pageSize: 20 })).resolves.toEqual(
      listResponse,
    );
    await expect(
      granolaApi.getNoteDetail({
        noteId: "not_1234567890ABCD",
        includeTranscript: true,
      }),
    ).resolves.toEqual(detail);
    await expect(
      granolaApi.getAgentConversationGranolaNote({
        conversationId: "conversation-1",
      }),
    ).resolves.toBeNull();
    await expect(
      granolaApi.assignAgentConversationGranolaNote({
        conversationId: "conversation-1",
        projectId: "project-1",
        noteId: "not_1234567890ABCD",
        title: "Planning sync",
      }),
    ).resolves.toEqual(bound);
    await expect(
      granolaApi.refreshAgentConversationGranolaNote({
        conversationId: "conversation-1",
      }),
    ).resolves.toEqual(bound);
    await expect(
      granolaApi.clearAgentConversationGranolaNote({
        conversationId: "conversation-1",
      }),
    ).resolves.toBeNull();

    expect(typedInvoke).toHaveBeenNthCalledWith(
      1,
      "list_granola_notes",
      { input: { pageSize: 20 } },
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      2,
      "get_granola_note_detail",
      {
        input: {
          noteId: "not_1234567890ABCD",
          includeTranscript: true,
        },
      },
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      4,
      "assign_agent_conversation_granola_note",
      {
        input: {
          conversationId: "conversation-1",
          projectId: "project-1",
          noteId: "not_1234567890ABCD",
          title: "Planning sync",
        },
      },
      expect.any(Object),
    );
  });

  it("parses the camelCase settings response", () => {
    const parsed = GranolaIntegrationSettingsSchema.parse({
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      lastValidatedAt: "2026-06-17T12:00:00Z",
      lastError: null,
      updatedAt: "2026-06-17T12:00:00Z",
    });

    expect(parsed.hasApiToken).toBe(true);
    expect(parsed.validationStatus).toBe("valid");
  });
});
