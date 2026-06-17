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

interface AgentsConversationSideRegionsProps {
  activeConversation: AgentConversation | null;
  activeProjectBaseBranch: string | null;
  activeWorkspace: AgentConversationWorkspace | null;
  activeWorkspaceFreshness: AgentConversationWorkspaceFreshness | undefined;
  artifactWidthCss: string;
  chatDockElement: HTMLDivElement | null;
  focusedIdeationSessionId: string | null;
  hasAutoOpenArtifacts: boolean;
  isArtifactResizing: boolean;
  openArtifactTab: (conversationId: string, tab: AgentArtifactTab) => void;
  panelDockElement: HTMLDivElement | null;
  publishFocusRequest: AgentPublishFocusRequest | null;
  publishingConversationId: string | null;
  selectedConversationId: string | null;
  setArtifactPaneVisibility: (conversationId: string, isOpen: boolean) => void;
  setArtifactTaskMode: (conversationId: string, mode: AgentTaskArtifactMode) => void;
  setTerminalPanelDockElement: (element: HTMLDivElement | null) => void;
  terminalUnavailableReason: string | null;
  onFocusVerificationSession: (parentSessionId: string, childSessionId: string) => void;
  onPublishWorkspace: (conversationId: string) => Promise<void>;
  onResizeReset: (event: ReactMouseEvent) => void;
  onResizeStart: (event: ReactMouseEvent) => void;
  onSelectArtifact: (tab: AgentArtifactTab) => void;
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
  isArtifactResizing,
  openArtifactTab,
  panelDockElement,
  publishFocusRequest,
  publishingConversationId,
  selectedConversationId,
  setArtifactPaneVisibility,
  setArtifactTaskMode,
  setTerminalPanelDockElement,
  terminalUnavailableReason,
  onFocusVerificationSession,
  onPublishWorkspace,
  onResizeReset,
  onResizeStart,
  onSelectArtifact,
}: AgentsConversationSideRegionsProps) {
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
          hasAutoOpenArtifacts={hasAutoOpenArtifacts}
          artifactWidthCss={artifactWidthCss}
          isArtifactResizing={isArtifactResizing}
          onResizeStart={onResizeStart}
          onResizeReset={onResizeReset}
          onTabChange={onSelectArtifact}
          onTaskModeChange={(mode) =>
            setArtifactTaskMode(selectedConversationId, mode)
          }
          onPublishWorkspace={onPublishWorkspace}
          isPublishingWorkspace={publishingConversationId === selectedConversationId}
          publishFocusRequest={publishFocusRequest}
          onFocusVerificationSession={onFocusVerificationSession}
          onClose={() => setArtifactPaneVisibility(selectedConversationId, false)}
          terminalUnavailableReason={terminalUnavailableReason}
          setTerminalPanelDockElement={setTerminalPanelDockElement}
        />
      ) : null}
      <AgentsTerminalRegion
        conversationId={selectedConversationId}
        workspace={activeWorkspace}
        terminalUnavailableReason={terminalUnavailableReason}
        hasAutoOpenArtifacts={hasAutoOpenArtifacts}
        chatDockElement={chatDockElement}
        panelDockElement={panelDockElement}
        onOpenArtifactTab={openArtifactTab}
      />
    </>
  );
}
