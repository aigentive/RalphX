import type { AgentConversationWorkspace } from "@/api/chat";
import type { AgentSidebarPublicationState } from "@/stores/agentSessionStore";
import type { Project } from "@/types/project";
import type { AgentConversation } from "./agentConversations";

export const PUBLICATION_STATE_OPTIONS: Array<{
  value: AgentSidebarPublicationState;
  label: string;
}> = [
  { value: "active", label: "Active" },
  { value: "draft", label: "Draft" },
  { value: "merged", label: "Merged" },
  { value: "closed", label: "Closed" },
  { value: "uncommitted", label: "Uncommitted" },
  { value: "unpushed", label: "Unpushed" },
];

export function getSidebarPublicationState(
  workspace: AgentConversationWorkspace | null | undefined
): AgentSidebarPublicationState {
  const prStatus = normalizeStatus(workspace?.publicationPrStatus);
  const pushStatus = normalizeStatus(workspace?.publicationPushStatus);

  if (prStatus === "merged") return "merged";
  if (prStatus === "closed") return "closed";
  if (pushStatus === "needs_agent") return "uncommitted";
  if (
    pushStatus === "pending" ||
    pushStatus === "failed" ||
    pushStatus === "description_failed"
  ) {
    return "unpushed";
  }
  if (prStatus === "draft") return "draft";

  return "active";
}

export function getSidebarPublicationLabel(
  state: AgentSidebarPublicationState
): string | null {
  if (state === "active") {
    return null;
  }
  return (
    PUBLICATION_STATE_OPTIONS.find((option) => option.value === state)?.label.toLowerCase() ??
    null
  );
}

export function getSidebarPublicationLabelForWorkspace(
  workspace: AgentConversationWorkspace | null | undefined,
  state: AgentSidebarPublicationState = getSidebarPublicationState(workspace)
): string | null {
  if (state === "active" || state === "uncommitted" || state === "unpushed") {
    const supervisionLabel = getSidebarSupervisionLabel(workspace);
    if (supervisionLabel) {
      return supervisionLabel;
    }
  }
  return getSidebarPublicationLabel(state);
}

export function getSidebarPublicationGroupLabel(
  state: AgentSidebarPublicationState
): string {
  return PUBLICATION_STATE_OPTIONS.find((option) => option.value === state)?.label ?? "Active";
}

export function getConversationRefDisplay(
  conversation: AgentConversation,
  project: Project,
  workspace: AgentConversationWorkspace | null | undefined
): {
  kind: "pull-request" | "branch";
  label: string;
} {
  if (workspace?.publicationPrNumber != null) {
    return {
      kind: "pull-request",
      label: `PR #${workspace.publicationPrNumber}`,
    };
  }

  return {
    kind: "branch",
    label:
      workspace?.baseRef ??
      workspace?.baseDisplayName ??
      project.baseBranch ??
      (conversation.contextType === "project" ? "master" : "base"),
  };
}

export function shouldShowConversationForPublicationFilters(
  workspace: AgentConversationWorkspace | null | undefined,
  selectedStates: AgentSidebarPublicationState[]
): boolean {
  if (selectedStates.length === 0) {
    return false;
  }
  return selectedStates.includes(getSidebarPublicationState(workspace));
}

function normalizeStatus(status: string | null | undefined): string | null {
  const normalized = status?.trim().toLowerCase();
  return normalized || null;
}

function getSidebarSupervisionLabel(
  workspace: AgentConversationWorkspace | null | undefined
): string | null {
  const supervisionStatus = normalizeStatus(workspace?.prSupervisionStatus);
  if (supervisionStatus === "fixing" || supervisionStatus === "publishing") {
    return "fixing";
  }
  if (supervisionStatus === "blocked") {
    return "blocked";
  }
  if (supervisionStatus === "held" || supervisionStatus === "paused") {
    return "paused";
  }
  if (
    supervisionStatus === "waiting" ||
    supervisionStatus === "waiting_for_checks"
  ) {
    return "waiting";
  }
  if (supervisionStatus === "monitoring" && workspace?.prAutoMergeCurrent === true) {
    return "auto-merge";
  }
  return null;
}
