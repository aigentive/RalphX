import { z } from "zod";

// Managed Team HTTP responses use `#[serde(rename_all = "camelCase")]`.
// Keep these schemas aligned to that explicit wire contract rather than the
// repository's usual snake_case default.
export const ManagedTeamSessionSchema = z.object({
  id: z.string(),
  projectId: z.string(),
  coordinatorConversationId: z.string(),
  status: z.string(),
  configuredConcurrency: z.number(),
  effectiveConcurrency: z.number(),
  automaticWakeLimit: z.number(),
  version: z.number(),
  createdAt: z.string(),
  updatedAt: z.string(),
});

export const ManagedTeamMemberSchema = z.object({
  id: z.string(),
  teamId: z.string(),
  name: z.string(),
  normalizedName: z.string(),
  canonicalAgentName: z.string(),
  roleSummary: z.string(),
  status: z.string(),
  generation: z.number(),
});

export const ManagedTeamStatusSchema = z.object({
  session: ManagedTeamSessionSchema,
  members: z.array(ManagedTeamMemberSchema),
  usage: z.object({
    tokens: z.number(),
    costMicros: z.number(),
    members: z.array(z.object({
      memberId: z.string().nullable(),
      tokens: z.number(),
      costMicros: z.number(),
    })),
  }),
});

export const ManagedTeamAssignmentSchema = z.object({
  assignmentId: z.string(),
  agentRunId: z.string(),
  member: ManagedTeamMemberSchema,
});

export const ManagedTeamRosterSchema = z.array(ManagedTeamMemberSchema);
