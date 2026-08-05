import {
  Suspense,
  type MouseEvent as ReactMouseEvent,
} from "react";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
} from "@/api/chat";
import { lazyWithRetry } from "@/lib/lazy-with-retry";
import { ResizeHandle } from "@/components/ui/ResizeHandle";
import { cn } from "@/lib/utils";
import type {
  AgentArtifactTab,
  AgentRuntimeSelection,
  AgentTaskArtifactMode,
} from "@/stores/agentSessionStore";

import { preloadAgentsArtifactPane } from "./agentArtifactPanePreload";
import { useResolvedAgentArtifactState } from "./agentArtifactState";
import type { AgentConversation } from "./agentConversations";
import { useAfterPaintMounted } from "./agentDeferredFrame";
import { AgentsTerminalDockHost } from "./AgentsTerminalRegion";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import type { AgentPublishSubTabRequest } from "./agentPublishSubTab";
import type { AgentWorkspacePublishAttempt } from "./useAgentWorkspacePublisher";
import type { AgentTaskArtifactFocusRequest } from "./agentTaskArtifactFocus";
import type { AgentTaskRuntimeContextType } from "./agentTaskRuntimeContext";
import type {
  AgentsChatFocus,
  AutomationRunFocusOptions,
  FocusedArtifactIdeationSession,
} from "./agentChatFocus";

export const AGENTS_ARTIFACT_MIN_WIDTH = 600;
export const AGENTS_CHAT_MIN_WIDTH = 600;

const LazyAgentsArtifactPane = lazyWithRetry(() =>
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
  activeWorkspaceError: Error | null;
  activeWorkspaceFreshness: AgentConversationWorkspaceFreshness | undefined;
  projectBaseBranch: string | null;
  focusedIdeationSession: FocusedArtifactIdeationSession | null;
  automationRunFocusTarget: Extract<
    AgentsChatFocus,
    { type: "automation_run" }
  > | null;
  hasAutoOpenArtifacts: boolean;
  artifactWidthCss: string;
  isArtifactResizing: boolean;
  onResizeStart: (event: ReactMouseEvent) => void;
  onResizeReset: (event: ReactMouseEvent) => void;
  onTabChange: (tab: AgentArtifactTab) => void;
  onHideTab: (
    tab: AgentArtifactTab,
    availableTabs: readonly AgentArtifactTab[],
  ) => void;
  onShowTab: (tab: AgentArtifactTab) => void;
  onOpenPublish: () => void;
  onRetryActiveWorkspace: () => void;
  onOpenAutomation?: (automationId: string) => void;
  onTaskModeChange: (mode: AgentTaskArtifactMode) => void;
  onPublishWorkspace: (conversationId: string) => Promise<void>;
  isPublishingWorkspace: boolean;
  publishAttempt: AgentWorkspacePublishAttempt | null;
  publishFocusRequest: AgentPublishFocusRequest | null;
  publishSubTabRequest: AgentPublishSubTabRequest | null;
  taskFocusRequest: AgentTaskArtifactFocusRequest | null;
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
  onFocusWorkspaceReview: (
    conversationId: string,
    runtimeHint?: AgentRuntimeSelection,
  ) => void;
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
  activeWorkspaceError,
  activeWorkspaceFreshness,
  projectBaseBranch,
  focusedIdeationSession,
  automationRunFocusTarget,
  hasAutoOpenArtifacts,
  artifactWidthCss,
  isArtifactResizing,
  onResizeStart,
  onResizeReset,
  onTabChange,
  onHideTab,
  onShowTab,
  onOpenPublish,
  onRetryActiveWorkspace,
  onOpenAutomation,
  onTaskModeChange,
  onPublishWorkspace,
  isPublishingWorkspace,
  publishAttempt,
  publishFocusRequest,
  publishSubTabRequest,
  taskFocusRequest,
  onConversationModeSwitched,
  onFocusIdeationSessionForConversation,
  onFocusAutomationRun,
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
  const workspaceConversationId = workspace?.conversationId ?? conversationId;

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
                    activeWorkspaceError={activeWorkspaceError}
                    activeWorkspaceFreshness={activeWorkspaceFreshness}
                    projectBaseBranch={projectBaseBranch}
                    focusedIdeationSession={focusedIdeationSession}
                    automationRunFocusTarget={automationRunFocusTarget}
                    activeTab={artifactState.activeTab}
                    hiddenTabs={artifactState.hiddenTabs}
                    taskMode={artifactState.taskMode}
                    onTabChange={onTabChange}
                    onHideTab={onHideTab}
                    onShowTab={onShowTab}
                    onOpenPublish={onOpenPublish}
                    onRetryActiveWorkspace={onRetryActiveWorkspace}
                    {...(onOpenAutomation ? { onOpenAutomation } : {})}
                    onTaskModeChange={onTaskModeChange}
                    onPublishWorkspace={onPublishWorkspace}
                    isPublishingWorkspace={isPublishingWorkspace}
                    publishAttempt={publishAttempt}
                    publishFocusRequest={publishFocusRequest}
                    publishSubTabRequest={publishSubTabRequest}
                    taskFocusRequest={taskFocusRequest}
                    onConversationModeSwitched={onConversationModeSwitched}
                    onFocusIdeationSessionForConversation={
                      onFocusIdeationSessionForConversation
                    }
                    onFocusAutomationRun={onFocusAutomationRun}
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
            conversationId={workspaceConversationId}
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
