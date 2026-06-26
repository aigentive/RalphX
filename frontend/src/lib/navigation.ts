import type { InfiniteData } from "@tanstack/react-query";
import type { AgentSidebarConversationGroup } from "@/api/chat";
import { getQueryClient } from "@/lib/queryClient";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useIdeationStore } from "@/stores/ideationStore";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";

const AGENT_SIDEBAR_CONVERSATION_QUERY_KEY = ["agents", "sidebar-conversations"] as const;

/**
 * Navigate to a specific ideation session.
 * When the standalone Ideation page is enabled, switches the main view to
 * "ideation" and selects the target session.
 *
 * When the standalone Ideation page is disabled, routes to the Agents surface.
 * If the linked Agent conversation is already present in the sidebar cache,
 * selects it directly; otherwise focuses the owning project and shows Agents.
 *
 * If the session belongs to a different project, pre-writes the target
 * project's view/session maps when using the standalone page and calls selectProject so the App.tsx
 * effect handles the rest (RESTORE phase reads our pre-written values).
 * Safe to call from any current view.
 */
export function navigateToIdeationSession(sessionId: string): void {
  const standaloneIdeationEnabled = useUiStore.getState().featureFlags.ideationPage;
  const session = useIdeationStore.getState().sessions[sessionId];

  if (!session) {
    console.warn(
      `navigateToIdeationSession: session "${sessionId}" not found in store — falling back to ${standaloneIdeationEnabled ? "direct Ideation navigation" : "Agents navigation"}`,
    );
    if (standaloneIdeationEnabled) {
      useUiStore.getState().setCurrentView("ideation");
      useIdeationStore.getState().setActiveSession(sessionId);
    } else {
      navigateToAgentsForIdeationSession(sessionId);
    }
    return;
  }

  const { activeProjectId } = useProjectStore.getState();
  const targetProjectId = session.projectId;

  if (!standaloneIdeationEnabled) {
    navigateToAgentsForIdeationSession(sessionId, targetProjectId);
    return;
  }

  if (activeProjectId !== null && activeProjectId !== targetProjectId) {
    // Cross-project navigation: pre-write maps so the App.tsx effect reads
    // the correct view and session during its RESTORE phase, then trigger
    // the project switch via selectProject.
    const uiState = useUiStore.getState();
    useUiStore.setState({
      viewByProject: { ...uiState.viewByProject, [targetProjectId]: "ideation" },
      sessionByProject: {
        ...uiState.sessionByProject,
        [targetProjectId]: sessionId,
      },
    });
    useProjectStore.getState().selectProject(targetProjectId);
    return;
  }

  // Same-project navigation: fast path.
  useUiStore.getState().setCurrentView("ideation");
  useIdeationStore.getState().setActiveSession(sessionId);
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

  if (projectId) {
    if (linkedConversation) {
      const agentSessionState = useAgentSessionStore.getState();
      agentSessionState.selectConversation(
        projectId,
        linkedConversation.conversationId,
      );
      agentSessionState.setArtifactTab(linkedConversation.conversationId, "plan");
    } else {
      useAgentSessionStore.getState().setFocusedProject(projectId);
    }

    const activeProjectId = useProjectStore.getState().activeProjectId;
    if (activeProjectId !== projectId) {
      const uiState = useUiStore.getState();
      useUiStore.setState({
        viewByProject: { ...uiState.viewByProject, [projectId]: "agents" },
      });
      useProjectStore.getState().selectProject(projectId);
      return;
    }
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
