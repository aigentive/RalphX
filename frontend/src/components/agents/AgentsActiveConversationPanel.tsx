import { memo, useCallback, useEffect, useMemo, useState } from "react";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceMode,
} from "@/api/chat";
import {
  IntegratedChatPanel,
  type IntegratedChatComposerRenderProps,
} from "@/components/Chat/IntegratedChatPanel";
import { buildStoreKey } from "@/lib/chat-context-registry";
import type {
  AgentArtifactTab,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";

import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import { AgentComposerProjectLine, AgentComposerSurface } from "./AgentComposerSurface";
import { AgentConversationBaseLine } from "./AgentConversationBaseLine";
import { AgentsChatHeaderController } from "./AgentsChatHeaderController";
import {
  AGENT_CONVERSATION_MODE_OPTIONS,
} from "./agentConversationMode";
import {
  AGENT_MODEL_OPTIONS,
  AGENT_PROVIDER_OPTIONS,
} from "./agentOptions";
import { AgentProviderSettingsButton } from "./AgentProviderSettingsButton";
import {
  buildAgentProviderAvailabilityOptions,
  getProviderAvailabilityMessage,
  supportedEffortsForProvider,
} from "./agentProviderAvailability";
import { AgentsTerminalDockHost } from "./AgentsTerminalRegion";
import type { IdeationArtifactTab } from "./agentArtifactTabs";

const AGENTS_CHAT_CONTENT_WIDTH_CLASS = "max-w-[980px]";

interface AgentComposerOption {
  id: string;
  label: string;
  description?: string;
  disabled?: boolean;
  disabledReason?: string;
}

interface AgentsActiveConversationPanelProps {
  activeConversation: AgentConversation;
  activeConversationMode: AgentConversationWorkspaceMode | null;
  activeConversationModeLocked: boolean;
  activeProjectId: string;
  activeProjectOptions: AgentComposerOption[];
  activeWorkspace: AgentConversationWorkspace | null;
  attachedIdeationSessionId: string | null;
  availableArtifactTabs: readonly IdeationArtifactTab[];
  hasAutoOpenArtifacts: boolean;
  normalizedActiveRuntime: AgentRuntimeSelection;
  onActiveConversationModeChange: (mode: AgentConversationWorkspaceMode) => void;
  onActiveModelChange: (modelId: string) => void;
  onAgentUserMessageSent: (event: {
    content: string;
    result: { conversationId: string };
  }) => void;
  onOpenPublishPane: () => void;
  onPreloadArtifacts: () => void;
  onPublishWorkspace: (conversationId: string) => Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => Promise<void>;
  onSelectArtifact: (tab: AgentArtifactTab) => void;
  onToggleArtifacts: (conversationId: string) => void;
  publishShortcutLabel: string;
  publishingConversationId: string | null;
  selectedConversationId: string;
  setTerminalChatDockElement: (element: HTMLDivElement | null) => void;
  switchingConversationModeId: string | null;
  terminalUnavailableReason: string | null;
}

type AgentsChatFocus =
  | { type: "workspace" }
  | { type: "ideation"; sessionId: string };

export const AgentsActiveConversationPanel = memo(function AgentsActiveConversationPanel({
  activeConversation,
  activeConversationMode,
  activeConversationModeLocked,
  activeProjectId,
  activeProjectOptions,
  activeWorkspace,
  attachedIdeationSessionId,
  availableArtifactTabs,
  hasAutoOpenArtifacts,
  normalizedActiveRuntime,
  onActiveConversationModeChange,
  onActiveModelChange,
  onAgentUserMessageSent,
  onOpenPublishPane,
  onPreloadArtifacts,
  onPublishWorkspace,
  onRenameConversation,
  onSelectArtifact,
  onToggleArtifacts,
  publishShortcutLabel,
  publishingConversationId,
  selectedConversationId,
  setTerminalChatDockElement,
  switchingConversationModeId,
  terminalUnavailableReason,
}: AgentsActiveConversationPanelProps) {
  const [chatFocus, setChatFocus] = useState<AgentsChatFocus>({ type: "workspace" });

  useEffect(() => {
    setChatFocus({ type: "workspace" });
  }, [selectedConversationId]);

  const handleChildSessionNavigate = useCallback((sessionId: string) => {
    setChatFocus({ type: "ideation", sessionId });
  }, []);

  const handleReturnToWorkspaceChat = useCallback(() => {
    setChatFocus({ type: "workspace" });
  }, []);

  const focusedIdeationSessionId =
    chatFocus.type === "ideation" ? chatFocus.sessionId : null;
  const panelIdeationSessionId =
    focusedIdeationSessionId ??
    (activeConversation.contextType === "ideation" ? activeConversation.contextId : undefined);
  const isFocusedChildIdeation = Boolean(focusedIdeationSessionId);
  const panelStoreKeyOverride = useMemo(() => {
    if (focusedIdeationSessionId) {
      return buildStoreKey("ideation", focusedIdeationSessionId);
    }
    return getAgentConversationStoreKey(activeConversation);
  }, [activeConversation, focusedIdeationSessionId]);

  return (
    <div className="flex-1 min-w-0 h-full flex flex-col">
      <div className="min-h-0 flex-1">
        <IntegratedChatPanel
          key={`${selectedConversationId}:${chatFocus.type}:${focusedIdeationSessionId ?? "workspace"}`}
          projectId={activeProjectId}
          {...(panelIdeationSessionId
            ? { ideationSessionId: panelIdeationSessionId }
            : {})}
          {...(!isFocusedChildIdeation
            ? { conversationIdOverride: selectedConversationId }
            : {})}
          selectedTaskIdOverride={null}
          storeContextKeyOverride={panelStoreKeyOverride}
          {...(!isFocusedChildIdeation && activeConversation.contextType === "project"
            ? { agentProcessContextIdOverride: selectedConversationId }
            : {})}
          {...(!isFocusedChildIdeation
            ? {
                sendOptions: {
                  conversationId: selectedConversationId,
                  providerHarness: normalizedActiveRuntime.provider,
                  modelId: normalizedActiveRuntime.modelId,
                },
              }
            : {})}
          onUserMessageSent={onAgentUserMessageSent}
          onChildSessionNavigate={handleChildSessionNavigate}
          hideHeaderSessionControls
          hideSessionToolbar
          surfaceBackground="var(--bg-base)"
          contentWidthClassName={AGENTS_CHAT_CONTENT_WIDTH_CLASS}
          {...(!isFocusedChildIdeation
            ? {
                inputContainerClassName:
                  "shrink-0 bg-transparent px-4 pb-4 pt-3",
                renderComposer: (composerProps: IntegratedChatComposerRenderProps) => (
                  <>
                    <AgentComposerSurface
                      dataTestId="agents-conversation-composer"
                      actionTestId="agents-conversation-submit"
                      onSend={composerProps.onSend}
                      onStop={composerProps.onStop}
                      agentStatus={composerProps.agentStatus}
                      isSubmitting={composerProps.isSending}
                      isReadOnly={composerProps.isReadOnly}
                      autoFocus={composerProps.autoFocus}
                      placeholder="Ask the agent to plan, build, debug, or review something"
                      showHelperText={false}
                      hasQueuedMessages={composerProps.hasQueuedMessages}
                      onEditLastQueued={composerProps.onEditLastQueued}
                      attachments={composerProps.attachments}
                      enableAttachments={composerProps.enableAttachments}
                      onFilesSelected={composerProps.onFilesSelected}
                      onRemoveAttachment={composerProps.onRemoveAttachment}
                      attachmentsUploading={composerProps.attachmentsUploading}
                      {...(composerProps.value !== undefined
                        ? {
                            value: composerProps.value,
                            onChange: composerProps.onChange,
                          }
                        : {})}
                      {...(composerProps.questionMode !== undefined
                        ? { questionMode: composerProps.questionMode }
                        : {})}
                      submitLabel="Send"
                      {...(activeConversationMode
                        ? {
                            mode: {
                              value: activeConversationMode,
                              onValueChange: (value: string) =>
                                onActiveConversationModeChange(
                                  value as AgentConversationWorkspaceMode,
                                ),
                              options: AGENT_CONVERSATION_MODE_OPTIONS,
                              disabled:
                                activeConversationModeLocked ||
                                composerProps.agentStatus !== "idle" ||
                                switchingConversationModeId === selectedConversationId,
                            },
                          }
                        : {})}
                      project={{
                        value: activeProjectId,
                        onValueChange: () => undefined,
                        options: activeProjectOptions,
                        placeholder: "Current project",
                        disabled: true,
                      }}
                      provider={{
                        value: normalizedActiveRuntime.provider,
                        onValueChange: () => undefined,
                        options: AGENT_PROVIDER_OPTIONS,
                        disabled: true,
                      }}
                      model={{
                        value: normalizedActiveRuntime.modelId,
                        onValueChange: onActiveModelChange,
                        options: AGENT_MODEL_OPTIONS[normalizedActiveRuntime.provider],
                      }}
                    />
                    <div className="mt-2 flex w-full flex-wrap items-center justify-between gap-2 px-2">
                      <AgentComposerProjectLine
                        value={activeProjectId}
                        onValueChange={() => undefined}
                        options={activeProjectOptions}
                        placeholder="Current project"
                        disabled
                      />
                      <AgentConversationBaseLine
                        workspace={activeWorkspace}
                      />
                    </div>
                  </>
                ),
              }
            : {})}
          {...(!isFocusedChildIdeation && activeConversation.contextType === "project" && attachedIdeationSessionId
            ? { additionalQuestionSessionIds: [attachedIdeationSessionId] }
            : {})}
          headerContent={
            <AgentsChatHeaderController
              conversation={activeConversation}
              workspace={activeWorkspace}
              availableArtifactTabs={availableArtifactTabs}
              modelDisplay={{
                id: normalizedActiveRuntime.modelId,
                label: normalizedActiveRuntime.modelId,
              }}
              {...(isFocusedChildIdeation
                ? {
                    focusReturnLabel: "Workspace chat",
                    onReturnToWorkspaceChat: handleReturnToWorkspaceChat,
                  }
                : {})}
              hasAutoOpenArtifacts={hasAutoOpenArtifacts}
              terminalUnavailableReason={terminalUnavailableReason}
              onRenameConversation={onRenameConversation}
              onPublishWorkspace={onPublishWorkspace}
              onOpenPublishPane={onOpenPublishPane}
              onPreloadArtifacts={onPreloadArtifacts}
              publishShortcutLabel={publishShortcutLabel}
              isPublishingWorkspace={publishingConversationId === selectedConversationId}
              onToggleArtifacts={onToggleArtifacts}
              onSelectArtifact={onSelectArtifact}
            />
          }
          emptyState={<div />}
        />
      </div>
      <AgentsTerminalDockHost
        dock="chat"
        conversationId={selectedConversationId}
        workspace={activeWorkspace}
        terminalUnavailableReason={terminalUnavailableReason}
        hasAutoOpenArtifacts={hasAutoOpenArtifacts}
        setDockElement={setTerminalChatDockElement}
      />
    </div>
  );
});
