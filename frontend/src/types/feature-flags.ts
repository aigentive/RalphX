import { z } from "zod";

export const featureFlagsSchema = z.object({
  activityPage: z.boolean(),
  extensibilityPage: z.boolean(),
  ideationPage: z.boolean().default(false),
  automationsPage: z.boolean().default(true),
  battleMode: z.boolean().default(true),
  teamMode: z.boolean().default(false),
  atlassianOauth: z.boolean().default(false),
  ticketingDashboard: z.boolean().default(false),
  agentPersonas: z.boolean().default(false),
  agentConversationTeam: z.boolean().default(false),
  agentConversationWorkflows: z.boolean().default(false),
  composerFolderReferences: z.boolean().default(false),
  standaloneConversations: z.boolean().default(false),
  agentConversationAutopilot: z.boolean().default(false),
});

/**
 * Compatibility type for persisted frontend defaults that predate this additive
 * flag. Parsed feature-flag responses always populate `agentPersonas` as false.
 */
export type FeatureFlags = Omit<
  z.infer<typeof featureFlagsSchema>,
  | "agentPersonas"
  | "agentConversationTeam"
  | "agentConversationWorkflows"
  | "composerFolderReferences"
  | "standaloneConversations"
  | "agentConversationAutopilot"
> & {
  agentPersonas?: boolean;
  agentConversationTeam?: boolean;
  agentConversationWorkflows?: boolean;
  composerFolderReferences?: boolean;
  standaloneConversations?: boolean;
  agentConversationAutopilot?: boolean;
};
