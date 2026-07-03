import { z } from "zod";

export const featureFlagsSchema = z.object({
  activityPage: z.boolean(),
  extensibilityPage: z.boolean(),
  ideationPage: z.boolean().default(false),
  battleMode: z.boolean().default(true),
  teamMode: z.boolean().default(false),
  atlassianOauth: z.boolean().default(false),
  ticketingDashboard: z.boolean().default(false),
});

export type FeatureFlags = z.infer<typeof featureFlagsSchema>;
