import { z } from "zod";

export const featureFlagsSchema = z.object({
  activityPage: z.boolean(),
  extensibilityPage: z.boolean(),
  automationsPage: z.boolean().default(true),
  atlassianOauth: z.boolean().default(false),
  ticketingDashboard: z.boolean().default(false),
  agentPersonas: z.boolean().default(false),
  agentConversationTeam: z.boolean().default(false),
  agentConversationWorkflows: z.boolean().default(false),
  standaloneConversations: z.boolean().default(false),
  agentConversationAutopilot: z.boolean().default(false),
  // Defaults ON (owner decision, 2026-08-03) to match the backend's shipped default.
  remoteEnvironments: z.boolean().default(true),
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
  | "standaloneConversations"
  | "agentConversationAutopilot"
  | "remoteEnvironments"
> & {
  agentPersonas?: boolean;
  agentConversationTeam?: boolean;
  agentConversationWorkflows?: boolean;
  standaloneConversations?: boolean;
  agentConversationAutopilot?: boolean;
  remoteEnvironments?: boolean;
};
