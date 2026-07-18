import type { AgentConversationWorkspaceMode } from "@/api/chat";

export const AGENT_START_MODE_OPTIONS: Array<{
  id: AgentConversationWorkspaceMode;
  label: string;
  description: string;
  requiresProject: boolean;
}> = [
  { id: "edit", label: "Agent", description: "Build, change, and review code in a branch.", requiresProject: true },
  { id: "review_pr", label: "Review PR", description: "Review a linked pull request.", requiresProject: true },
  { id: "plan", label: "Plan", description: "Draft and refine a plan before execution.", requiresProject: true },
  { id: "automation", label: "Automation", description: "Create and run a recurring agent workflow.", requiresProject: true },
  { id: "persona_builder", label: "Persona", description: "Build or refine a reusable agent persona.", requiresProject: false },
  { id: "chat", label: "Chat", description: "Ask read-only questions about the project.", requiresProject: false },
  { id: "ideation", label: "Ideation", description: "Plan work before creating tasks.", requiresProject: true },
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
        ...AGENT_START_MODE_OPTIONS.slice(0, 3),
        AUTOPILOT_OPTION,
        ...AGENT_START_MODE_OPTIONS.slice(3),
      ]
    : AGENT_START_MODE_OPTIONS;
}
