import { z } from "zod";

import { typedInvoke } from "@/lib/tauri";

export const GranolaValidationStatusSchema = z.enum([
  "not_configured",
  "pending",
  "valid",
  "invalid",
]);

export type GranolaValidationStatus = z.infer<
  typeof GranolaValidationStatusSchema
>;

export const GranolaIntegrationSettingsSchema = z.object({
  enabled: z.boolean(),
  hasApiToken: z.boolean(),
  validationStatus: GranolaValidationStatusSchema,
  lastValidatedAt: z.string().nullable().optional(),
  lastError: z.string().nullable().optional(),
  updatedAt: z.string(),
});

export type GranolaIntegrationSettings = z.infer<
  typeof GranolaIntegrationSettingsSchema
>;

export interface SaveGranolaIntegrationSettingsInput {
  apiToken?: string | null;
}

export const GranolaNoteSummarySchema = z.object({
  id: z.string(),
  title: z.string().nullable().optional(),
  url: z.string().nullable().optional(),
  summary: z.string().nullable().optional(),
  createdAt: z.string().nullable().optional(),
  updatedAt: z.string().nullable().optional(),
});

export type GranolaNoteSummary = z.infer<typeof GranolaNoteSummarySchema>;

export const GranolaTranscriptEntrySchema = z.object({
  speaker: z.string().nullable().optional(),
  text: z.string(),
  startMs: z.number().nullable().optional(),
  endMs: z.number().nullable().optional(),
});

export type GranolaTranscriptEntry = z.infer<typeof GranolaTranscriptEntrySchema>;

export const GranolaNoteDetailSchema = z.object({
  id: z.string(),
  title: z.string().nullable().optional(),
  url: z.string().nullable().optional(),
  summary: z.string().nullable().optional(),
  transcript: z.array(GranolaTranscriptEntrySchema).default([]),
});

export type GranolaNoteDetail = z.infer<typeof GranolaNoteDetailSchema>;

export const ListGranolaNotesResponseSchema = z.object({
  notes: z.array(GranolaNoteSummarySchema),
  hasMore: z.boolean(),
  cursor: z.string().nullable().optional(),
});

export const AgentConversationGranolaNoteSchema = z.object({
  conversationId: z.string(),
  projectId: z.string(),
  provider: z.literal("granola"),
  noteId: z.string(),
  noteUrl: z.string().nullable().optional(),
  title: z.string().nullable().optional(),
  summaryMarkdown: z.string().nullable().optional(),
  transcript: z.array(z.unknown()).default([]),
  includeTranscript: z.boolean(),
  lastRefreshedAt: z.string().nullable().optional(),
  refreshStatus: z.enum(["not_loaded", "loaded", "error"]),
  refreshError: z.string().nullable().optional(),
  assignedAt: z.string(),
  assignedFromMessageId: z.string().nullable().optional(),
  manuallyAssigned: z.boolean(),
  createdAt: z.string(),
  updatedAt: z.string(),
});

export type AgentConversationGranolaNote = z.infer<
  typeof AgentConversationGranolaNoteSchema
>;

export const AgentConversationGranolaNoteResponseSchema = z.object({
  note: AgentConversationGranolaNoteSchema.nullable().optional(),
});

export interface ListGranolaNotesInput {
  pageSize?: number;
  cursor?: string | null;
}

export interface GetGranolaNoteDetailInput {
  noteId: string;
  includeTranscript?: boolean;
}

export interface AgentConversationGranolaNoteConversationInput {
  conversationId: string;
}

export interface AssignAgentConversationGranolaNoteInput {
  conversationId: string;
  projectId?: string | null;
  noteId: string;
  title?: string | null;
  noteUrl?: string | null;
  summary?: string | null;
  includeTranscript?: boolean;
  refresh?: boolean;
}

export const granolaApi = {
  getSettings(): Promise<GranolaIntegrationSettings> {
    return typedInvoke(
      "get_granola_integration_settings",
      {},
      GranolaIntegrationSettingsSchema,
    );
  },

  saveSettings(
    input: SaveGranolaIntegrationSettingsInput,
  ): Promise<GranolaIntegrationSettings> {
    return typedInvoke(
      "save_granola_integration_settings",
      { input },
      GranolaIntegrationSettingsSchema,
    );
  },

  validate(): Promise<GranolaIntegrationSettings> {
    return typedInvoke(
      "validate_granola_integration_settings",
      {},
      GranolaIntegrationSettingsSchema,
    );
  },

  disconnect(): Promise<GranolaIntegrationSettings> {
    return typedInvoke(
      "save_granola_integration_settings",
      { input: { apiToken: "" } },
      GranolaIntegrationSettingsSchema,
    );
  },

  async listNotes(input: ListGranolaNotesInput = {}): Promise<{
    notes: GranolaNoteSummary[];
    hasMore: boolean;
    cursor?: string | null | undefined;
  }> {
    return typedInvoke(
      "list_granola_notes",
      { input },
      ListGranolaNotesResponseSchema,
    );
  },

  getNoteDetail(input: GetGranolaNoteDetailInput): Promise<GranolaNoteDetail> {
    return typedInvoke(
      "get_granola_note_detail",
      { input },
      GranolaNoteDetailSchema,
    );
  },

  async getAgentConversationGranolaNote(
    input: AgentConversationGranolaNoteConversationInput,
  ): Promise<AgentConversationGranolaNote | null> {
    const response = await typedInvoke(
      "get_agent_conversation_granola_note",
      { input },
      AgentConversationGranolaNoteResponseSchema,
    );
    return response.note ?? null;
  },

  async assignAgentConversationGranolaNote(
    input: AssignAgentConversationGranolaNoteInput,
  ): Promise<AgentConversationGranolaNote | null> {
    const response = await typedInvoke(
      "assign_agent_conversation_granola_note",
      { input },
      AgentConversationGranolaNoteResponseSchema,
    );
    return response.note ?? null;
  },

  async refreshAgentConversationGranolaNote(
    input: AgentConversationGranolaNoteConversationInput,
  ): Promise<AgentConversationGranolaNote | null> {
    const response = await typedInvoke(
      "refresh_agent_conversation_granola_note",
      { input },
      AgentConversationGranolaNoteResponseSchema,
    );
    return response.note ?? null;
  },

  async clearAgentConversationGranolaNote(
    input: AgentConversationGranolaNoteConversationInput,
  ): Promise<AgentConversationGranolaNote | null> {
    const response = await typedInvoke(
      "clear_agent_conversation_granola_note",
      { input },
      AgentConversationGranolaNoteResponseSchema,
    );
    return response.note ?? null;
  },
} as const;
