import type { LaunchRuntimeRoleKey } from "@/stores/agentSessionStore";

import type { AgentsChatFocus } from "./agentChatFocus";

export function getChatFocusRuntimeRole(
  chatFocus: AgentsChatFocus,
): LaunchRuntimeRoleKey | null {
  switch (chatFocus.type) {
    case "workspace_review":
      return "workspace_reviewer";
    case "workspace_repair":
      return "workspace_repair";
    case "pr_fixer":
      return "pr_fixer";
    default:
      return null;
  }
}

export function getChatFocusRuntimeLabel(
  chatFocus: AgentsChatFocus,
): "Reviewer" | "Fixer" | "PR Fixer" | null {
  switch (chatFocus.type) {
    case "workspace_review":
      return "Reviewer";
    case "workspace_repair":
      return "Fixer";
    case "pr_fixer":
      return "PR Fixer";
    default:
      return null;
  }
}

export function getChatFocusRuntimeTag(
  chatFocus: AgentsChatFocus,
): "REV" | "FIX" | null {
  return chatFocus.type === "workspace_review"
    ? "REV"
    : chatFocus.type === "workspace_repair" || chatFocus.type === "pr_fixer"
      ? "FIX"
      : null;
}
