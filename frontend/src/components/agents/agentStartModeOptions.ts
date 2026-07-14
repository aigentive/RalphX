import type { AgentConversationWorkspaceMode } from "@/api/chat";

export const AGENT_START_MODE_OPTIONS: Array<{
  id: AgentConversationWorkspaceMode;
  label: string;
  description: string;
}> = [
  { id: "edit", label: "Agent", description: "Build, change, and review code in a branch." },
  { id: "review_pr", label: "Review PR", description: "Review a linked pull request." },
  { id: "plan", label: "Plan", description: "Draft and refine a plan before execution." },
  { id: "automation", label: "Automation", description: "Create and run a recurring agent workflow." },
  { id: "chat", label: "Chat", description: "Ask read-only questions about the project." },
  { id: "ideation", label: "Ideation", description: "Plan work before creating tasks." },
];
