import type { QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import {
  automationsApi,
  type AutomationDetail,
  type AutomationJudgeState,
  type AutomationRunStatus,
} from "@/api/automations";
import type { AgentConversationWorkspaceMode } from "@/api/chat";
import { automationKeys } from "@/hooks/useAutomations";
import {
  useAgentSessionStore,
  type AgentArtifactTab,
} from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";
import { seedAgentArtifactTab } from "@/components/agents/agentArtifactState";
import {
  getAutomationConversationTabPolicy,
  type AutomationConversationTabId,
} from "./automationConversationTabPolicy";

export interface AutomationRunOpenTarget {
  projectId: string;
  automationId: string;
  runId: string;
  conversationId: string;
  setupConversationId?: string | null;
  runStatus?: AutomationRunStatus | null;
  judgeState?: AutomationJudgeState | null;
  planPhase?: boolean | null;
  planArtifactId?: string | null;
  prNumber?: number | null;
  prUrl?: string | null;
}

export interface RequestAutomationRunOpenOptions {
  fallback?: "detail" | "clear-selection";
  onOpenAutomationDetail?: (automationId: string) => void;
  tabHint?: AutomationConversationTabId;
}

interface ResolvedAutomationRunOpenTarget {
  projectId: string;
  automationId: string;
  runId: string;
  conversationId: string;
  setupConversationId: string;
  runStatus: AutomationRunStatus | null;
  judgeState: AutomationJudgeState | null;
  workspaceMode: AgentConversationWorkspaceMode | null;
  hasPlanArtifact: boolean;
  hasPullRequest: boolean;
}

export interface AutomationRunOpenResult {
  applied: boolean;
  reason?: "stale" | "not_found";
}

let latestAutomationRunOpenRequestId = 0;
const inFlightDetailByAutomationId = new Map<string, Promise<AutomationDetail>>();

export function resetAutomationRunOpenRequestStateForTests() {
  latestAutomationRunOpenRequestId = 0;
  inFlightDetailByAutomationId.clear();
}

function applyAgentsShell(projectId: string) {
  const agentSession = useAgentSessionStore.getState();
  agentSession.setFocusedProject(projectId);

  const projectState = useProjectStore.getState();
  if (projectState.activeProjectId !== projectId) {
    projectState.selectProject(projectId);
  }

  useUiStore.getState().setCurrentView("agents");
}

function workspaceModeForRun(planPhase: boolean | null | undefined) {
  return planPhase ? "plan" : null;
}

function hasPullRequest(target: Pick<AutomationRunOpenTarget, "prNumber" | "prUrl">) {
  return Boolean(target.prNumber || target.prUrl);
}

function targetFromKnownSetup(
  target: AutomationRunOpenTarget,
): ResolvedAutomationRunOpenTarget | null {
  if (!target.setupConversationId) {
    return null;
  }
  return {
    projectId: target.projectId,
    automationId: target.automationId,
    runId: target.runId,
    conversationId: target.conversationId,
    setupConversationId: target.setupConversationId,
    runStatus: target.runStatus ?? null,
    judgeState: target.judgeState ?? null,
    workspaceMode: workspaceModeForRun(target.planPhase),
    hasPlanArtifact: Boolean(target.planArtifactId),
    hasPullRequest: hasPullRequest(target),
  };
}

function targetFromDetail(
  target: AutomationRunOpenTarget,
  detail: AutomationDetail,
): ResolvedAutomationRunOpenTarget | null {
  const setupConversationId = detail.automation.setupConversationId;
  if (!setupConversationId) {
    return null;
  }
  const run = detail.runs.find((candidate) => candidate.id === target.runId);
  if (!run || run.conversationId !== target.conversationId) {
    return null;
  }
  return {
    projectId: target.projectId,
    automationId: detail.automation.id,
    runId: run.id,
    conversationId: run.conversationId,
    setupConversationId,
    runStatus: run.status,
    judgeState: run.judgeState,
    workspaceMode: workspaceModeForRun(run.planPhase),
    hasPlanArtifact: Boolean(run.planArtifactId),
    hasPullRequest: hasPullRequest(run),
  };
}

function fetchAutomationDetail(
  queryClient: QueryClient,
  automationId: string,
): Promise<AutomationDetail> {
  const existing = inFlightDetailByAutomationId.get(automationId);
  if (existing) {
    return existing;
  }

  const request = queryClient
    .ensureQueryData({
      queryKey: automationKeys.detail(automationId),
      queryFn: () => automationsApi.get(automationId),
      staleTime: 5_000,
    })
    .finally(() => {
      inFlightDetailByAutomationId.delete(automationId);
    });
  inFlightDetailByAutomationId.set(automationId, request);
  return request;
}

function defaultTabForResolvedTarget(
  target: ResolvedAutomationRunOpenTarget,
  options: RequestAutomationRunOpenOptions,
): AgentArtifactTab {
  return getAutomationConversationTabPolicy({
    surface: "run",
    runStatus: target.runStatus,
    judgeState: target.judgeState,
    workspaceMode: target.workspaceMode,
    availability: {
      hasPlanArtifact: target.hasPlanArtifact,
      hasPullRequest: target.hasPullRequest,
    },
    ...(options.tabHint !== undefined && { tabHint: options.tabHint }),
  }).defaultTab as AgentArtifactTab;
}

function applyResolvedRunFocus(
  target: ResolvedAutomationRunOpenTarget,
  options: RequestAutomationRunOpenOptions,
) {
  const agentSession = useAgentSessionStore.getState();
  const seededTab = defaultTabForResolvedTarget(target, options);

  agentSession.selectConversation(target.projectId, target.setupConversationId);
  agentSession.requestAutomationRunFocus(target.setupConversationId, {
    projectId: target.projectId,
    automationId: target.automationId,
    runId: target.runId,
    conversationId: target.conversationId,
    runStatus: target.runStatus,
    judgeState: target.judgeState,
    workspaceMode: target.workspaceMode,
    hasPlanArtifact: target.hasPlanArtifact,
    hasPullRequest: target.hasPullRequest,
    seededTab,
  });
  seedAgentArtifactTab(target.setupConversationId, seededTab, false);

  useChatStore
    .getState()
    .setActiveConversation(
      `project:${target.projectId}`,
      target.setupConversationId,
    );
}

function handleUnresolvedTarget(
  automationId: string,
  options: RequestAutomationRunOpenOptions,
): AutomationRunOpenResult {
  toast.error("Could not open automation run.");
  if (options.fallback === "clear-selection") {
    useAgentSessionStore.getState().clearSelection();
  } else {
    options.onOpenAutomationDetail?.(automationId);
  }
  return { applied: false, reason: "not_found" };
}

export async function requestAutomationRunOpen(
  queryClient: QueryClient,
  target: AutomationRunOpenTarget,
  options: RequestAutomationRunOpenOptions = {},
): Promise<AutomationRunOpenResult> {
  const requestId = latestAutomationRunOpenRequestId + 1;
  latestAutomationRunOpenRequestId = requestId;
  applyAgentsShell(target.projectId);

  let resolved = targetFromKnownSetup(target);
  if (!resolved) {
    let detail: AutomationDetail;
    try {
      detail = await fetchAutomationDetail(queryClient, target.automationId);
    } catch {
      if (latestAutomationRunOpenRequestId !== requestId) {
        return { applied: false, reason: "stale" };
      }
      return handleUnresolvedTarget(target.automationId, options);
    }
    if (latestAutomationRunOpenRequestId !== requestId) {
      return { applied: false, reason: "stale" };
    }
    resolved = targetFromDetail(target, detail);
  }

  if (!resolved) {
    return handleUnresolvedTarget(target.automationId, options);
  }

  if (latestAutomationRunOpenRequestId !== requestId) {
    return { applied: false, reason: "stale" };
  }

  applyResolvedRunFocus(resolved, options);
  return { applied: true };
}
