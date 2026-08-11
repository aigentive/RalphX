import { typedInvoke } from "@/lib/tauri";
import { z } from "zod";

export const AgentLaneSchema = z.enum([
  "ideation_primary",
  "ideation_verifier",
  "ideation_subagent",
  "ideation_verifier_subagent",
  "execution_worker",
  "execution_reviewer",
  "execution_reexecutor",
  "execution_merger",
  "execution_branch_updater",
]);

export type AgentLane = z.infer<typeof AgentLaneSchema>;

export const KNOWN_HARNESSES = ["claude", "codex"] as const;

export type KnownHarness = (typeof KNOWN_HARNESSES)[number];

export const HarnessSchema = z.string().min(1);

export type Harness = z.infer<typeof HarnessSchema>;

export const AgentLaneSettingsResponseSchema = z.object({
  projectId: z.string().nullable().optional(),
  lane: AgentLaneSchema,
  harness: HarnessSchema,
  model: z.string().nullable().optional(),
  effort: z.string().nullable().optional(),
  approvalPolicy: z.string().nullable().optional(),
  sandboxMode: z.string().nullable().optional(),
  updatedAt: z.string(),
});

export type AgentLaneSettingsResponse = z.infer<
  typeof AgentLaneSettingsResponseSchema
>;

export const AgentHarnessAvailabilityResponseSchema = z.object({
  projectId: z.string().nullable().optional(),
  lane: AgentLaneSchema,
  configuredHarness: HarnessSchema.nullable().optional(),
  effectiveHarness: HarnessSchema,
  binaryPath: z.string().nullable().optional(),
  binaryFound: z.boolean(),
  probeSucceeded: z.boolean(),
  available: z.boolean(),
  missingCoreExecFeatures: z.array(z.string()),
  error: z.string().nullable().optional(),
});

export type AgentHarnessAvailabilityResponse = z.infer<
  typeof AgentHarnessAvailabilityResponseSchema
>;

export interface AgentHarnessLaneView {
  lane: AgentLane;
  row: AgentLaneSettingsResponse | null;
  configuredHarness: Harness | null;
  effectiveHarness: Harness;
  binaryPath: string | null;
  binaryFound: boolean;
  probeSucceeded: boolean;
  available: boolean;
  missingCoreExecFeatures: string[];
  error: string | null;
}

export interface UpdateAgentHarnessLaneInput {
  projectId: string | null;
  lane: AgentLane;
  harness: Harness;
  model?: string | null;
  effort?: string | null;
  approvalPolicy?: string | null;
  sandboxMode?: string | null;
}

export interface GetAgentHarnessOptions {
  refreshRuntime?: boolean;
}

export const IDEATION_LANES: AgentLane[] = [
  "ideation_primary",
  "ideation_subagent",
];

export const EXECUTION_LANES: AgentLane[] = [
  "execution_worker",
  "execution_reviewer",
  "execution_reexecutor",
  "execution_merger",
  "execution_branch_updater",
];

export const AGENT_LANES: AgentLane[] = [...IDEATION_LANES, ...EXECUTION_LANES];

export const defaultAgentHarnessLanes: AgentHarnessLaneView[] =
  AGENT_LANES.map((lane) => ({
    lane,
    row: null,
    configuredHarness: null,
    effectiveHarness: "claude",
    binaryPath: null,
    binaryFound: false,
    probeSucceeded: false,
    available: false,
    missingCoreExecFeatures: [],
    error: null,
  }));

export function mergeAgentHarnessState(
  rows: AgentLaneSettingsResponse[],
  availability: AgentHarnessAvailabilityResponse[],
): AgentHarnessLaneView[] {
  return AGENT_LANES.map((lane) => {
    const row = rows.find((entry) => entry.lane === lane) ?? null;
    const status = availability.find((entry) => entry.lane === lane);

    return {
      lane,
      row,
      configuredHarness:
        row?.harness ?? status?.configuredHarness ?? null,
      effectiveHarness:
        status?.effectiveHarness ?? row?.harness ?? "claude",
      binaryPath: status?.binaryPath ?? null,
      binaryFound: status?.binaryFound ?? false,
      probeSucceeded: status?.probeSucceeded ?? false,
      available: status?.available ?? false,
      missingCoreExecFeatures: status?.missingCoreExecFeatures ?? [],
      error: status?.error ?? null,
    };
  });
}

export const agentHarnessApi = {
  async get(
    projectId: string | null,
    options: GetAgentHarnessOptions = {},
  ): Promise<AgentHarnessLaneView[]> {
    const input = {
      projectId,
      refreshRuntime: options.refreshRuntime ?? false,
    };
    const [rows, availability] = await Promise.all([
      typedInvoke(
        "get_agent_lane_settings",
        { projectId },
        z.array(AgentLaneSettingsResponseSchema),
      ),
      typedInvoke(
        "get_agent_harness_availability",
        { input },
        z.array(AgentHarnessAvailabilityResponseSchema),
      ),
    ]);

    return mergeAgentHarnessState(rows, availability);
  },

  update(
    input: UpdateAgentHarnessLaneInput,
  ): Promise<AgentLaneSettingsResponse> {
    return typedInvoke(
      "update_agent_lane_settings",
      { input },
      AgentLaneSettingsResponseSchema,
    );
  },
} as const;
