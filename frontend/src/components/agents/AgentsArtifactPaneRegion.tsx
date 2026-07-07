import {
  lazy,
  Suspense,
  type MouseEvent as ReactMouseEvent,
} from "react";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
} from "@/api/chat";
import { ResizeHandle } from "@/components/ui/ResizeHandle";
import { cn } from "@/lib/utils";
import type {
  AgentArtifactTab,
  AgentTaskArtifactMode,
} from "@/stores/agentSessionStore";

import { preloadAgentsArtifactPane } from "./agentArtifactPanePreload";
import { useResolvedAgentArtifactState } from "./agentArtifactState";
import type { AgentConversation } from "./agentConversations";
import { useAfterPaintMounted } from "./agentDeferredFrame";
import { AgentsTerminalDockHost } from "./AgentsTerminalRegion";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import type { AgentTaskArtifactFocusRequest } from "./agentTaskArtifactFocus";
import type { AgentTaskRuntimeContextType } from "./agentTaskRuntimeContext";

export const AGENTS_ARTIFACT_MIN_WIDTH = 600;
export const AGENTS_CHAT_MIN_WIDTH = 600;

const LazyAgentsArtifactPane = lazy(() =>
  preloadAgentsArtifactPane().then((module) => ({ default: module.AgentsArtifactPane })),
);

function AgentArtifactPaneLoadingShell() {
  return (
    <div
      className="flex h-full min-h-[220px] items-center justify-center p-6 text-center text-sm font-medium text-[var(--text-primary)]"
      data-testid="agents-artifact-pane-loading"
    >
      Loading panel...
    </div>
  );
}

interface AgentsArtifactPaneRegionProps {
  conversationId: string;
  conversation: AgentConversation;
  workspace: AgentConversationWorkspace | null;
  activeWorkspaceFreshness: AgentConversationWorkspaceFreshness | undefined;
  projectBaseBranch: string | null;
  focusedIdeationSessionId: string | null;
  hasAutoOpenArtifacts: boolean;
  artifactWidthCss: string;
  isArtifactResizing: boolean;
  onResizeStart: (event: ReactMouseEvent) => void;
  onResizeReset: (event: ReactMouseEvent) => void;
  onTabChange: (tab: AgentArtifactTab) => void;
  onOpenPublish: () => void;
  onOpenAutomation?: (automationId: string) => void;
  onTaskModeChange: (mode: AgentTaskArtifactMode) => void;
  onPublishWorkspace: (conversationId: string) => Promise<void>;
  isPublishingWorkspace: boolean;
  publishFocusRequest: AgentPublishFocusRequest | null;
  taskFocusRequest: AgentTaskArtifactFocusRequest | null;
  onFocusVerificationSession: (parentSessionId: string, childSessionId: string) => void;
  onFocusWorkspaceReview: (conversationId: string) => void;
  onFocusTaskRuntime: (
    taskId: string,
    contextType: AgentTaskRuntimeContextType
  ) => void;
  onTaskArtifactSelectionChange: (taskId: string | null) => void;
  onClose: () => void;
  terminalArchivedReason: string | null;
  terminalUnavailableReason: string | null;
  setTerminalPanelDockElement: (element: HTMLDivElement | null) => void;
}

export function AgentsArtifactPaneRegion({
  conversationId,
  conversation,
  workspace,
  activeWorkspaceFreshness,
  projectBaseBranch,
  focusedIdeationSessionId,
  hasAutoOpenArtifacts,
  artifactWidthCss,
  isArtifactResizing,
  onResizeStart,
  onResizeReset,
  onTabChange,
  onOpenPublish,
  onOpenAutomation,
  onTaskModeChange,
  onPublishWorkspace,
  isPublishingWorkspace,
  publishFocusRequest,
  taskFocusRequest,
  onFocusVerificationSession,
  onFocusWorkspaceReview,
  onFocusTaskRuntime,
  onTaskArtifactSelectionChange,
  onClose,
  terminalArchivedReason,
  terminalUnavailableReason,
  setTerminalPanelDockElement,
}: AgentsArtifactPaneRegionProps) {
  const { artifactState, artifactPaneOpen } = useResolvedAgentArtifactState(
    conversationId,
    hasAutoOpenArtifacts,
  );
  const contentMounted = useAfterPaintMounted(artifactPaneOpen);
  const shouldRenderArtifactContent = artifactPaneOpen || contentMounted;

  return (
    <>
      {artifactPaneOpen ? (
        <div className="max-lg:hidden">
          <ResizeHandle
            isResizing={isArtifactResizing}
            onMouseDown={onResizeStart}
            onDoubleClick={onResizeReset}
            testId="agents-artifact-resize-handle"
          />
        </div>
      ) : null}
      <div
        className={cn(
          "h-full shrink-0 overflow-hidden",
          artifactPaneOpen &&
            "max-lg:absolute max-lg:inset-y-0 max-lg:right-0 max-lg:z-20 max-lg:!w-[min(100%,420px)] max-lg:!min-w-0 max-lg:!max-w-none",
        )}
        style={{
          width: artifactPaneOpen ? artifactWidthCss : "0px",
          minWidth: artifactPaneOpen ? AGENTS_ARTIFACT_MIN_WIDTH : 0,
          maxWidth: artifactPaneOpen
            ? `calc(100% - ${AGENTS_CHAT_MIN_WIDTH}px)`
            : 0,
          opacity: artifactPaneOpen ? 1 : 0,
          pointerEvents: artifactPaneOpen ? "auto" : "none",
          transition: "none",
        }}
        data-testid="agents-artifact-resizable-pane"
      >
        <div className="flex h-full min-h-0 flex-col">
          {shouldRenderArtifactContent ? (
            <div className="min-h-0 flex-1">
              {contentMounted ? (
                <Suspense fallback={<AgentArtifactPaneLoadingShell />}>
                  <LazyAgentsArtifactPane
                    conversation={conversation}
                    workspace={workspace}
                    activeWorkspaceFreshness={activeWorkspaceFreshness}
                    projectBaseBranch={projectBaseBranch}
                    focusedIdeationSessionId={focusedIdeationSessionId}
                    activeTab={artifactState.activeTab}
                    taskMode={artifactState.taskMode}
                    onTabChange={onTabChange}
                    onOpenPublish={onOpenPublish}
                    {...(onOpenAutomation ? { onOpenAutomation } : {})}
                    onTaskModeChange={onTaskModeChange}
                    onPublishWorkspace={onPublishWorkspace}
                    isPublishingWorkspace={isPublishingWorkspace}
                    publishFocusRequest={publishFocusRequest}
                    taskFocusRequest={taskFocusRequest}
                    onFocusVerificationSession={onFocusVerificationSession}
                    onFocusWorkspaceReview={onFocusWorkspaceReview}
                    onFocusTaskRuntime={onFocusTaskRuntime}
                    onTaskArtifactSelectionChange={onTaskArtifactSelectionChange}
                    onClose={onClose}
                  />
                </Suspense>
              ) : (
                <AgentArtifactPaneLoadingShell />
              )}
            </div>
          ) : null}
          <AgentsTerminalDockHost
            dock="panel"
            conversationId={conversationId}
            workspace={workspace}
            terminalArchivedReason={terminalArchivedReason}
            terminalUnavailableReason={terminalUnavailableReason}
            hasAutoOpenArtifacts={hasAutoOpenArtifacts}
            setDockElement={setTerminalPanelDockElement}
          />
        </div>
      </div>
    </>
  );
}
