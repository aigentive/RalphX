import type { InfiniteData } from "@tanstack/react-query";

import type { AgentSidebarConversationGroup } from "@/api/chat";
import { getQueryClient } from "@/lib/queryClient";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useIdeationStore } from "@/stores/ideationStore";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";

const AGENT_SIDEBAR_CONVERSATION_QUERY_KEY = ["agents", "sidebar-conversations"] as const;

type AgentTaskMode = "graph" | "kanban";

/** Opens an ideation session through its linked Agent conversation when available. */
export function navigateToIdeationSession(sessionId: string): void {
  const session = useIdeationStore.getState().sessions[sessionId];
  navigateToAgentsForIdeationSession(sessionId, session?.projectId);
}

/** Select an Agent conversation by its durable conversation identity. */
export function navigateToAgentConversation(
  projectId: string,
  conversationId: string,
): void {
  const agentSessionState = useAgentSessionStore.getState();
  agentSessionState.selectConversation(projectId, conversationId);
  useChatStore
    .getState()
    .setActiveConversation(`project:${projectId}`, conversationId);
  navigateToAgentsProject(projectId);
}

/** Opens an Agent conversation with its Plan artifact selected. */
export function navigateToAgentPlanConversation(
  projectId: string,
  conversationId: string,
): void {
  useAgentSessionStore.getState().setArtifactTab(conversationId, "plan");
  navigateToAgentConversation(projectId, conversationId);
}

/** Opens an Agent conversation's Tasks artifact at a specific task and view mode. */
export function navigateToAgentTask(
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

function navigateToAgentsForIdeationSession(
  sessionId: string,
  targetProjectId?: string | null,
): void {
  const linkedConversation = findCachedLinkedAgentConversation(sessionId);
  const projectId =
    linkedConversation?.projectId ??
    targetProjectId ??
    useProjectStore.getState().activeProjectId;

  if (linkedConversation) {
    useAgentSessionStore
      .getState()
      .setArtifactTab(linkedConversation.conversationId, "plan");
    navigateToAgentConversation(projectId ?? linkedConversation.projectId, linkedConversation.conversationId);
    return;
  }

  if (projectId) {
    useAgentSessionStore.getState().setFocusedProject(projectId);
    navigateToAgentsProject(projectId);
    return;
  }

  useUiStore.getState().setCurrentView("agents");
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

function findCachedLinkedAgentConversation(
  sessionId: string,
): { conversationId: string; projectId: string } | null {
  const queryClient = getQueryClient();
  const sidebarQueries = queryClient.getQueriesData<
    InfiniteData<AgentSidebarConversationGroup>
  >({ queryKey: AGENT_SIDEBAR_CONVERSATION_QUERY_KEY });

  for (const [, data] of sidebarQueries) {
    for (const page of data?.pages ?? []) {
      for (const row of page.rows) {
        if (row.workspace?.linkedIdeationSessionId !== sessionId) {
          continue;
        }
        return {
          conversationId: row.workspace.conversationId,
          projectId: row.workspace.projectId,
        };
      }
    }
  }

  return null;
}
