import type { AgentConversationWorkspaceMode } from "@/api/chat";

export const AGENT_START_MODE_OPTIONS: Array<{
  id: AgentConversationWorkspaceMode;
  label: string;
  description: string;
  requiresProject: boolean;
}> = [
  { id: "plan", label: "Plan", description: "Draft and refine a plan before execution.", requiresProject: true },
  { id: "edit", label: "Agent", description: "Build, change, and review code in a branch.", requiresProject: true },
  { id: "review_pr", label: "Review PR", description: "Review a remote GitHub PR through its local checkout and propose a user-approved GitHub review.", requiresProject: true },
  { id: "chat", label: "Ask", description: "Ask read-only questions about the project.", requiresProject: false },
  { id: "automation", label: "Automation", description: "Create and run a recurring agent workflow.", requiresProject: true },
  { id: "persona_builder", label: "Persona", description: "Build or refine a reusable agent persona.", requiresProject: false },
];

export const PRIMARY_AGENT_START_MODE_IDS: readonly AgentConversationWorkspaceMode[] = [
  "plan",
  "edit",
  "review_pr",
  "chat",
];

const AUTOPILOT_OPTION = {
  id: "autopilot" as const,
  label: "Autopilot",
  description: "Plan, create tasks, and start execution with minimal supervision.",
  requiresProject: true,
};

export function buildAgentStartModeOptions({
  autopilotEnabled,
}: {
  autopilotEnabled: boolean;
}) {
  return autopilotEnabled
    ? [
        ...AGENT_START_MODE_OPTIONS.slice(0, 5),
        AUTOPILOT_OPTION,
        ...AGENT_START_MODE_OPTIONS.slice(5),
      ]
    : AGENT_START_MODE_OPTIONS;
}
