import { z } from "zod";

import { typedInvoke } from "@/lib/tauri";

export const AgentComposerEntrySchema = z.object({
  path: z.string(),
  kind: z.enum(["file", "directory"]),
  parentPath: z.string().nullable().optional(),
});

export type AgentComposerEntry = z.infer<typeof AgentComposerEntrySchema>;

export const SearchAgentComposerEntriesResponseSchema = z.object({
  entries: z.array(AgentComposerEntrySchema),
  truncated: z.boolean(),
});

export type SearchAgentComposerEntriesResponse = z.infer<
  typeof SearchAgentComposerEntriesResponseSchema
>;

export const AgentComposerPlanReferenceSchema = z.object({
  sessionId: z.string(),
  artifactId: z.string(),
  title: z.string().nullable().optional(),
  status: z.enum(["draft", "approved", "accepted"]),
  artifactVersion: z.number(),
  updatedAt: z.string(),
  approvedAt: z.string().nullable().optional(),
});

export type AgentComposerPlanReference = z.infer<
  typeof AgentComposerPlanReferenceSchema
>;

export const SearchAgentComposerPlanReferencesResponseSchema = z.object({
  plans: z.array(AgentComposerPlanReferenceSchema),
  truncated: z.boolean(),
});

export type SearchAgentComposerPlanReferencesResponse = z.infer<
  typeof SearchAgentComposerPlanReferencesResponseSchema
>;

export const AgentComposerSkillSchema = z.object({
  id: z.string(),
  name: z.string(),
  displayName: z.string().nullable().optional(),
  description: z.string().nullable().optional(),
  source: z.enum(["ralphx-internal", "harness-native"]),
  providerHarness: z.string().nullable().optional(),
  scope: z.string().nullable().optional(),
  invocationKind: z.string(),
  invocationValue: z.string(),
  enabled: z.boolean(),
  sourcePath: z.string().nullable().optional(),
});

export type AgentComposerSkill = z.infer<typeof AgentComposerSkillSchema>;

export const ListAgentComposerSkillsResponseSchema = z.object({
  skills: z.array(AgentComposerSkillSchema),
});

export type ListAgentComposerSkillsResponse = z.infer<
  typeof ListAgentComposerSkillsResponseSchema
>;

export interface SearchAgentComposerEntriesInput {
  projectId: string;
  conversationId?: string | null;
  query: string;
  limit?: number;
}

export interface SearchAgentComposerPlanReferencesInput {
  projectId: string;
  query: string;
  limit?: number;
}

export interface ListAgentComposerSkillsInput {
  projectId: string;
  conversationId?: string | null;
  providerHarness?: string | null;
  mode?: string | null;
}

export const agentComposerApi = {
  searchEntries(
    input: SearchAgentComposerEntriesInput,
  ): Promise<SearchAgentComposerEntriesResponse> {
    return typedInvoke(
      "search_agent_composer_entries",
      { input },
      SearchAgentComposerEntriesResponseSchema,
    );
  },

  listSkills(
    input: ListAgentComposerSkillsInput,
  ): Promise<ListAgentComposerSkillsResponse> {
    return typedInvoke(
      "list_agent_composer_skills",
      { input },
      ListAgentComposerSkillsResponseSchema,
    );
  },

  searchPlanReferences(
    input: SearchAgentComposerPlanReferencesInput,
  ): Promise<SearchAgentComposerPlanReferencesResponse> {
    return typedInvoke(
      "search_agent_composer_plan_references",
      { input },
      SearchAgentComposerPlanReferencesResponseSchema,
    );
  },
} as const;
