import type { MouseEvent as ReactMouseEvent } from "react";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
} from "@/api/chat";
import type {
  AgentArtifactTab,
  AgentTaskArtifactMode,
} from "@/stores/agentSessionStore";

import type { AgentConversation } from "./agentConversations";
import { AgentsArtifactPaneRegion } from "./AgentsArtifactPaneRegion";
import { AgentsTerminalRegion } from "./AgentsTerminalRegion";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import type { AgentPublishSubTabRequest } from "./agentPublishSubTab";
import type { AgentTaskArtifactFocusRequest } from "./agentTaskArtifactFocus";
import type { AgentTaskRuntimeContextType } from "./agentTaskRuntimeContext";
import type {
  AgentsChatFocus,
  AutomationRunFocusOptions,
} from "./agentChatFocus";

interface AgentsConversationSideRegionsProps {
  activeConversation: AgentConversation | null;
  activeProjectBaseBranch: string | null;
  activeWorkspace: AgentConversationWorkspace | null;
  activeWorkspaceFreshness: AgentConversationWorkspaceFreshness | undefined;
  artifactWidthCss: string;
  chatDockElement: HTMLDivElement | null;
  focusedIdeationSessionId: string | null;
  hasAutoOpenArtifacts: boolean;
  hideArtifactTab: (
    conversationId: string,
    tab: AgentArtifactTab,
    availableTabs: readonly AgentArtifactTab[],
  ) => void;
  isArtifactResizing: boolean;
  openArtifactTab: (conversationId: string, tab: AgentArtifactTab) => void;
  automationRunFocusTarget: Extract<
    AgentsChatFocus,
    { type: "automation_run" }
  > | null;
  panelDockElement: HTMLDivElement | null;
  publishFocusRequest: AgentPublishFocusRequest | null;
  publishSubTabRequest: AgentPublishSubTabRequest | null;
  publishingConversationId: string | null;
  selectedConversationId: string | null;
  setArtifactPaneVisibility: (conversationId: string, isOpen: boolean) => void;
  setArtifactTaskMode: (conversationId: string, mode: AgentTaskArtifactMode) => void;
  showArtifactTab: (conversationId: string, tab: AgentArtifactTab) => void;
  setTerminalPanelDockElement: (element: HTMLDivElement | null) => void;
  taskArtifactFocusRequest: AgentTaskArtifactFocusRequest | null;
  terminalArchivedReason: string | null;
  terminalUnavailableReason: string | null;
  onConversationModeSwitched: (
    conversationId: string,
    mode: AgentConversationWorkspace["mode"],
    workspace: AgentConversationWorkspace | null
  ) => void;
  onFocusIdeationSessionForConversation: (
    conversationId: string,
    sessionId: string
  ) => void;
  onFocusVerificationSession: (
    parentSessionId: string,
    childSessionId: string
  ) => void;
  onFocusAutomationRun: (
    automationId: string,
    runId: string,
    conversationId: string,
    options?: AutomationRunFocusOptions,
  ) => void;
  onFocusWorkspaceReview: (conversationId: string) => void;
  onFocusTaskRuntime: (
    taskId: string,
    contextType: AgentTaskRuntimeContextType
  ) => void;
  onOpenAutomation?: (automationId: string) => void;
  onOpenPublish: () => void;
  onPublishWorkspace: (conversationId: string) => Promise<void>;
  onResizeReset: (event: ReactMouseEvent) => void;
  onResizeStart: (event: ReactMouseEvent) => void;
  onSelectArtifact: (tab: AgentArtifactTab) => void;
  onTaskArtifactSelectionChange: (taskId: string | null) => void;
}

export function AgentsConversationSideRegions({
  activeConversation,
  activeProjectBaseBranch,
  activeWorkspace,
  activeWorkspaceFreshness,
  artifactWidthCss,
  chatDockElement,
  focusedIdeationSessionId,
  hasAutoOpenArtifacts,
  hideArtifactTab,
  isArtifactResizing,
  openArtifactTab,
  automationRunFocusTarget,
  panelDockElement,
  publishFocusRequest,
  publishSubTabRequest,
  publishingConversationId,
  selectedConversationId,
  setArtifactPaneVisibility,
  setArtifactTaskMode,
  showArtifactTab,
  setTerminalPanelDockElement,
  taskArtifactFocusRequest,
  terminalArchivedReason,
  terminalUnavailableReason,
  onConversationModeSwitched,
  onFocusIdeationSessionForConversation,
  onFocusAutomationRun,
  onFocusVerificationSession,
  onFocusWorkspaceReview,
  onFocusTaskRuntime,
  onOpenAutomation,
  onOpenPublish,
  onPublishWorkspace,
  onResizeReset,
  onResizeStart,
  onSelectArtifact,
  onTaskArtifactSelectionChange,
}: AgentsConversationSideRegionsProps) {
  const workspaceConversationId =
    activeWorkspace?.conversationId ?? selectedConversationId;

  return (
    <>
      {selectedConversationId && activeConversation ? (
        <AgentsArtifactPaneRegion
          conversationId={selectedConversationId}
          conversation={activeConversation}
          workspace={activeWorkspace}
          activeWorkspaceFreshness={activeWorkspaceFreshness}
          projectBaseBranch={activeProjectBaseBranch}
          focusedIdeationSessionId={focusedIdeationSessionId}
          automationRunFocusTarget={automationRunFocusTarget}
          hasAutoOpenArtifacts={hasAutoOpenArtifacts}
          artifactWidthCss={artifactWidthCss}
          isArtifactResizing={isArtifactResizing}
          onResizeStart={onResizeStart}
          onResizeReset={onResizeReset}
          onTabChange={onSelectArtifact}
          onHideTab={(tab, availableTabs) =>
            hideArtifactTab(selectedConversationId, tab, availableTabs)
          }
          onShowTab={(tab) => showArtifactTab(selectedConversationId, tab)}
          onOpenPublish={onOpenPublish}
          onTaskModeChange={(mode) =>
            setArtifactTaskMode(selectedConversationId, mode)
          }
          onPublishWorkspace={onPublishWorkspace}
          isPublishingWorkspace={publishingConversationId === selectedConversationId}
          publishFocusRequest={publishFocusRequest}
          publishSubTabRequest={publishSubTabRequest}
          taskFocusRequest={taskArtifactFocusRequest}
          onConversationModeSwitched={onConversationModeSwitched}
          onFocusIdeationSessionForConversation={
            onFocusIdeationSessionForConversation
          }
          onFocusVerificationSession={onFocusVerificationSession}
          onFocusWorkspaceReview={onFocusWorkspaceReview}
          onFocusTaskRuntime={onFocusTaskRuntime}
          onFocusAutomationRun={onFocusAutomationRun}
          {...(onOpenAutomation ? { onOpenAutomation } : {})}
          onTaskArtifactSelectionChange={onTaskArtifactSelectionChange}
          onClose={() => setArtifactPaneVisibility(selectedConversationId, false)}
          terminalArchivedReason={terminalArchivedReason}
          terminalUnavailableReason={terminalUnavailableReason}
          setTerminalPanelDockElement={setTerminalPanelDockElement}
        />
      ) : null}
      <AgentsTerminalRegion
        conversationId={workspaceConversationId}
        workspace={activeWorkspace}
        terminalArchivedReason={terminalArchivedReason}
        terminalUnavailableReason={terminalUnavailableReason}
        hasAutoOpenArtifacts={hasAutoOpenArtifacts}
        chatDockElement={chatDockElement}
        panelDockElement={panelDockElement}
        onOpenArtifactTab={openArtifactTab}
      />
    </>
  );
}
