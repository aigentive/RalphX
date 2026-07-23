import { toast } from "sonner";

import { ideationApi } from "@/api/ideation";
import { tasksApi } from "@/api/tasks";
import { revealAgentArtifactTab } from "@/components/agents/agentArtifactState";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";

export type AgentTaskMode = "graph" | "kanban";
export interface AgentTaskNavigationHints {
  projectId?: string | null;
  conversationId?: string | null;
}

let latestNavigationIntent = 0;

function claimNavigationIntent(): number {
  latestNavigationIntent += 1;
  return latestNavigationIntent;
}

function isLatestNavigationIntent(intent: number): boolean {
  return intent === latestNavigationIntent;
}

/** Opens an ideation session through its linked Agent conversation. */
export function navigateToIdeationSession(sessionId: string): void {
  void openIdeationInAgents(sessionId);
}

/**
 * Resolves and opens the owning Agent conversation for an ideation session.
 * Only the latest unresolved navigation intent is allowed to mutate selection.
 */
export async function openIdeationInAgents(sessionId: string): Promise<boolean> {
  const intent = claimNavigationIntent();
  // Sidebar rows are navigation hints only. The backend revalidates the
  // session-to-workspace link before any selection is allowed to change.
  const target = await resolveIdeationAgentWorkspace(sessionId);
  if (!isLatestNavigationIntent(intent)) return false;
  if (!target) {
    showAgentsFallback();
    toast.info("This ideation session is no longer linked to an Agent workspace.");
    return false;
  }

  applyAgentPlan(target.projectId, target.conversationId);
  return true;
}

/**
 * Resolves and opens the owning Agent conversation for a task.
 * Ownership is resolved before any project, artifact, or task selection changes.
 */
export async function openTaskInAgents(
  taskId: string,
  taskMode: AgentTaskMode,
  hints: AgentTaskNavigationHints = {},
): Promise<boolean> {
  const intent = claimNavigationIntent();
  // Callers may have stale project/conversation values from a deep link or
  // cached row. They are hints for compatibility, never ownership evidence.
  void hints;
  let workspace: Awaited<ReturnType<typeof tasksApi.resolveAgentWorkspace>> = null;

  try {
    workspace = await tasksApi.resolveAgentWorkspace(taskId);
  } catch {
    workspace = null;
  }

  if (!isLatestNavigationIntent(intent)) return false;
  if (!workspace) {
    showAgentsFallback();
    toast.info("Open the linked Agent conversation to view this task.");
    return false;
  }

  applyAgentTask(
    workspace.projectId,
    workspace.conversationId,
    taskId,
    taskMode,
  );
  return true;
}

/** Select an Agent conversation by its durable conversation identity. */
export function navigateToAgentConversation(
  projectId: string | null,
  conversationId: string,
): void {
  claimNavigationIntent();
  applyAgentConversation(projectId, conversationId);
}

function applyAgentConversation(
  projectId: string | null,
  conversationId: string,
): void {
  const agentSessionState = useAgentSessionStore.getState();
  agentSessionState.selectConversation(projectId, conversationId);
  if (projectId === null) {
    useChatStore
      .getState()
      .setActiveConversation(
        buildStoreKey("standalone", conversationId),
        conversationId,
      );
    useUiStore.getState().setCurrentView("agents");
    return;
  }
  useChatStore
    .getState()
    .setActiveConversation(`project:${projectId}`, conversationId);
  navigateToAgentsProject(projectId);
}

/** Opens an Agent conversation's Plan artifact. */
export function navigateToAgentPlan(
  projectId: string,
  conversationId: string,
): void {
  claimNavigationIntent();
  applyAgentPlan(projectId, conversationId);
}

function applyAgentPlan(projectId: string, conversationId: string): void {
  revealAgentArtifactTab(conversationId, "plan", false);
  applyAgentConversation(projectId, conversationId);
}

/** Opens an Agent conversation's Tasks artifact at a specific task and view mode. */
export function navigateToAgentTask(
  projectId: string,
  conversationId: string,
  taskId: string,
  taskMode: AgentTaskMode,
): void {
  claimNavigationIntent();
  applyAgentTask(projectId, conversationId, taskId, taskMode);
}

function applyAgentTask(
  projectId: string,
  conversationId: string,
  taskId: string,
  taskMode: AgentTaskMode,
): void {
  const agentSessionState = useAgentSessionStore.getState();
  agentSessionState.selectConversation(projectId, conversationId);
  agentSessionState.setTaskArtifactMode(conversationId, taskMode);
  agentSessionState.focusTaskArtifact(conversationId, taskId);
  useChatStore
    .getState()
    .setActiveConversation(`project:${projectId}`, conversationId);
  navigateToAgentsProject(projectId);
}

async function resolveIdeationAgentWorkspace(
  sessionId: string,
): Promise<{ conversationId: string; projectId: string } | null> {
  try {
    const workspace = await ideationApi.sessions.resolveAgentWorkspace(sessionId);
    return workspace
      ? {
          conversationId: workspace.conversationId,
          projectId: workspace.projectId,
        }
      : null;
  } catch {
    return null;
  }
}

function navigateToAgentsProject(projectId: string): void {
  const activeProjectId = useProjectStore.getState().activeProjectId;
  if (activeProjectId !== projectId) {
    const uiState = useUiStore.getState();
    useUiStore.setState({
      viewByProject: { ...uiState.viewByProject, [projectId]: "agents" },
    });
    useProjectStore.getState().selectProject(projectId);
    return;
  }

  useUiStore.getState().setCurrentView("agents");
}

/** Shows the retained root without disturbing the user's current Agent selection. */
function showAgentsFallback(): void {
  useUiStore.getState().setCurrentView("agents");
}
