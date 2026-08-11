import type { AgentConversationWorkspaceMode } from "@/api/chat";
import type { AgentRuntimeSelection } from "@/stores/agentSessionStore";
import type {
  AutomationJudgeState,
  AutomationRun,
  AutomationRunStatus,
} from "@/api/automations";
import type { AgentTaskRuntimeContextType } from "./agentTaskRuntimeContext";

export type AgentsChatFocus =
  | { type: "workspace" }
  | {
      type: "workspace_review";
      conversationId: string;
      runtimeHint?: AgentRuntimeSelection;
    }
  | { type: "workspace_repair"; conversationId: string }
  | { type: "pr_fixer"; conversationId: string }
  | { type: "ideation"; conversationId: string; sessionId: string }
  | {
      type: "verification";
      conversationId: string;
      parentSessionId: string;
      childSessionId: string;
    }
  | { type: "task_runtime"; taskId: string; contextType: AgentTaskRuntimeContextType }
  | {
      type: "automation_run";
      automationId: string;
      runId: string;
      conversationId: string;
    };

export function focusWorkspaceReview(
  current: AgentsChatFocus,
  conversationId: string,
  runtimeHint?: AgentRuntimeSelection,
): Extract<AgentsChatFocus, { type: "workspace_review" }> {
  if (
    current.type === "workspace_review" &&
    current.conversationId === conversationId
  ) {
    return runtimeHint ? { ...current, runtimeHint } : current;
  }
  return {
    type: "workspace_review",
    conversationId,
    ...(runtimeHint ? { runtimeHint } : {}),
  };
}

export type AgentsChatFocusType = AgentsChatFocus["type"];
export type AgentsChatFocusTone = "accent" | "warning";

export interface AutomationRunFocusOptions {
  runStatus: AutomationRunStatus | null;
  judgeState: AutomationJudgeState | null;
  workspaceMode: AgentConversationWorkspaceMode | null;
  hasPlanArtifact: boolean;
  hasPullRequest: boolean;
}

export interface AgentsChatFocusDisplay {
  type: Exclude<AgentsChatFocus["type"], "workspace">;
  label: string;
  description: string;
  tone: AgentsChatFocusTone;
}

export interface AgentsChatFocusSwitchOption {
  type: AgentsChatFocusType;
  label: string;
  description: string;
  tone?: AgentsChatFocusTone;
}

export function getAgentChatFocusSwitchOptions({
  mode,
  focusSwitcherIdeationSessionId,
  verificationFocusTarget,
  taskRuntimeFocusTarget,
  workspaceReviewFocusTarget,
  workspaceRepairFocusTarget,
  prFixerFocusTarget,
  automationRunFocusTarget,
  hasPlanArtifact,
}: {
  mode: AgentConversationWorkspaceMode | null;
  focusSwitcherIdeationSessionId: string | null;
  verificationFocusTarget: Extract<AgentsChatFocus, { type: "verification" }> | null;
  taskRuntimeFocusTarget: Extract<AgentsChatFocus, { type: "task_runtime" }> | null;
  workspaceReviewFocusTarget: Extract<AgentsChatFocus, { type: "workspace_review" }> | null;
  workspaceRepairFocusTarget: Extract<AgentsChatFocus, { type: "workspace_repair" }> | null;
  prFixerFocusTarget: Extract<AgentsChatFocus, { type: "pr_fixer" }> | null;
  automationRunFocusTarget: Extract<AgentsChatFocus, { type: "automation_run" }> | null;
  hasPlanArtifact: boolean;
}): AgentsChatFocusSwitchOption[] {
  const options: AgentsChatFocusSwitchOption[] = [
    {
      type: "workspace",
      label: "Workspace",
      description: "Show the workspace agent chat",
    },
  ];

  if (mode === "ideation" && focusSwitcherIdeationSessionId) {
    options.push({
      type: "ideation",
      label: "Ideation",
      description: "Show the attached ideation chat",
      tone: "accent",
    });
  }

  const canShowVerification =
    Boolean(verificationFocusTarget) &&
    (mode === "ideation" || (mode === "plan" && hasPlanArtifact));

  if (canShowVerification) {
    options.push({
      type: "verification",
      label: "Verification",
      description: "Show the verification agent chat",
      tone: "warning",
    });
  }

  if (workspaceReviewFocusTarget) {
    options.push({
      type: "workspace_review",
      label: "Review",
      description: "Show the Review chat",
      tone: "warning",
    });
  }

  if (workspaceRepairFocusTarget) {
    options.push({
      type: "workspace_repair",
      label: "Fixer",
      description: "Show the workspace fixer chat",
      tone: "warning",
    });
  }

  if (prFixerFocusTarget) {
    options.push({
      type: "pr_fixer",
      label: "PR Fixer",
      description: "Show the PR fixer chat",
      tone: "warning",
    });
  }

  if (taskRuntimeFocusTarget) {
    options.push({
      type: "task_runtime",
      label: "Task",
      description: "Show the task agent chat",
      tone: "accent",
    });
  }

  if (mode === "automation" && automationRunFocusTarget) {
    options.push({
      type: "automation_run",
      label: "Run",
      description: "Show the automation run chat",
      tone: "accent",
    });
  }

  return options;
}

export function getAutomationRunFocusOptions(
  run: AutomationRun,
): AutomationRunFocusOptions {
  return {
    runStatus: run.status,
    judgeState: run.judgeState,
    workspaceMode: run.planPhase ? "plan" : null,
    hasPlanArtifact: Boolean(run.planArtifactId),
    hasPullRequest: Boolean(run.prNumber || run.prUrl),
  };
}

export function latestVerificationChildSessionIdQueryKey(
  parentSessionId: string | null | undefined,
) {
  return [
    "agents",
    "chat-focus",
    "latest-child-session-id",
    parentSessionId,
    "verification",
  ] as const;
}

export function latestVerificationChildSessionData(
  parentSessionId: string,
  childSessionId: string | null,
) {
  return {
    sessionId: parentSessionId,
    purpose: "verification" as const,
    latestChildSessionId: childSessionId,
  };
}

export interface FocusedArtifactIdeationSession {
  conversationId: string;
  sessionId: string;
}

export function getConversationScopedChatFocus(
  chatFocus: AgentsChatFocus,
  conversationId: string | null,
): AgentsChatFocus {
  if (
    (chatFocus.type === "ideation" || chatFocus.type === "verification") &&
    chatFocus.conversationId !== conversationId
  ) {
    return { type: "workspace" };
  }
  return chatFocus;
}

export function getFocusedArtifactIdeationSession(
  chatFocus: AgentsChatFocus,
): FocusedArtifactIdeationSession | null {
  if (chatFocus.type === "ideation") {
    return {
      conversationId: chatFocus.conversationId,
      sessionId: chatFocus.sessionId,
    };
  }
  if (chatFocus.type === "verification") {
    return {
      conversationId: chatFocus.conversationId,
      sessionId: chatFocus.parentSessionId,
    };
  }
  return null;
}

export function getAgentsChatFocusDisplay(
  chatFocus: AgentsChatFocus,
): AgentsChatFocusDisplay | null {
  if (chatFocus.type === "ideation") {
    return {
      type: "ideation",
      label: "Ideation",
      description: "Focused on an ideation run",
      tone: "accent",
    };
  }

  if (chatFocus.type === "verification") {
    return {
      type: "verification",
      label: "Verification",
      description: "Focused on a verification run",
      tone: "warning",
    };
  }

  if (chatFocus.type === "task_runtime") {
    return {
      type: "task_runtime",
      label: "Task",
      description: "Focused on a task agent run",
      tone: "accent",
    };
  }

  if (chatFocus.type === "workspace_review") {
    return {
      type: "workspace_review",
      label: "Review",
      description: "Focused on a Review run",
      tone: "warning",
    };
  }

  if (chatFocus.type === "workspace_repair") {
    return {
      type: "workspace_repair",
      label: "Fixer",
      description: "Focused on a workspace fixer run",
      tone: "warning",
    };
  }

  if (chatFocus.type === "pr_fixer") {
    return {
      type: "pr_fixer",
      label: "PR Fixer",
      description: "Focused on a PR fixer run",
      tone: "warning",
    };
  }

  if (chatFocus.type === "automation_run") {
    return {
      type: "automation_run",
      label: "Run",
      description: "Focused on an automation run",
      tone: "accent",
    };
  }

  return null;
}

export function getFocusedChatSessionId(chatFocus: AgentsChatFocus): string | null {
  if (chatFocus.type === "ideation") {
    return chatFocus.sessionId;
  }
  if (chatFocus.type === "verification") {
    return chatFocus.childSessionId;
  }
  return null;
}

export function getFocusedWorkspaceReviewConversationId(
  chatFocus: AgentsChatFocus,
): string | null {
  if (chatFocus.type === "workspace_review") {
    return chatFocus.conversationId;
  }
  return null;
}

export function getFocusedFixerConversationId(
  chatFocus: AgentsChatFocus,
): string | null {
  if (
    chatFocus.type === "workspace_repair" ||
    chatFocus.type === "pr_fixer"
  ) {
    return chatFocus.conversationId;
  }
  return null;
}

export function getFocusedAutomationRunConversationId(
  chatFocus: AgentsChatFocus,
): string | null {
  if (chatFocus.type === "automation_run") {
    return chatFocus.conversationId;
  }
  return null;
}
