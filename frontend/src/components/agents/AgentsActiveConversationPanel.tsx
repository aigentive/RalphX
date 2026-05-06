import { memo, useMemo } from "react";
import { Lightbulb, MessageSquare, ShieldCheck } from "lucide-react";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceMode,
} from "@/api/chat";
import {
  IntegratedChatPanel,
  type IntegratedChatComposerRenderProps,
} from "@/components/Chat/IntegratedChatPanel";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { useAgentModels } from "@/hooks/useAgentModels";
import type {
  AgentArtifactTab,
  AgentProvider,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";

import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import {
  AgentComposerProjectLine,
  AgentComposerSurface,
  type ChatFocusFieldConfig,
} from "./AgentComposerSurface";
import { AgentConversationBaseLine } from "./AgentConversationBaseLine";
import {
  AgentsChatFocusBar,
} from "./AgentsChatHeader";
import { AgentsChatHeaderController } from "./AgentsChatHeaderController";
import {
  AGENT_CONVERSATION_MODE_OPTIONS,
} from "./agentConversationMode";
import {
  AGENT_PROVIDER_OPTIONS,
  agentEffortOptions,
  agentModelOptions,
} from "./agentOptions";
import { AgentsTerminalDockHost } from "./AgentsTerminalRegion";
import type { IdeationArtifactTab } from "./agentArtifactTabs";
import {
  getFocusedChatSessionId,
  type AgentsChatFocus,
  type AgentsChatFocusSwitchOption,
  type AgentsChatFocusType,
} from "./agentChatFocus";

const AGENTS_CHAT_CONTENT_WIDTH_CLASS = "max-w-[980px]";

interface AgentComposerOption {
  id: string;
  label: string;
  description?: string;
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
  chatFocus: AgentsChatFocus;
  chatFocusOptions: readonly AgentsChatFocusSwitchOption[];
  hasAutoOpenArtifacts: boolean;
  normalizedActiveRuntime: AgentRuntimeSelection;
  onActiveConversationModeChange: (mode: AgentConversationWorkspaceMode) => void;
  onActiveEffortChange: (effort: string) => void;
  onActiveModelChange: (modelId: string) => void;
  onAgentUserMessageSent: (event: {
    content: string;
    result: { conversationId: string };
  }) => void;
  onFocusIdeationSession: (sessionId: string) => void;
  onOpenPublishPane: () => void;
  onPreloadArtifacts: () => void;
  onPublishWorkspace: (conversationId: string) => Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => Promise<void>;
  onSelectArtifact: (tab: AgentArtifactTab) => void;
  onToggleArtifacts: (conversationId: string) => void;
  onSelectChatFocus: (type: AgentsChatFocusType) => void;
  publishShortcutLabel: string;
  publishingConversationId: string | null;
  selectedConversationId: string;
  setTerminalChatDockElement: (element: HTMLDivElement | null) => void;
  switchingConversationModeId: string | null;
  terminalUnavailableReason: string | null;
}

export const AgentsActiveConversationPanel = memo(function AgentsActiveConversationPanel({
  activeConversation,
  activeConversationMode,
  activeConversationModeLocked,
  activeProjectId,
  activeProjectOptions,
  activeWorkspace,
  attachedIdeationSessionId,
  availableArtifactTabs,
  chatFocus,
  chatFocusOptions,
  hasAutoOpenArtifacts,
  normalizedActiveRuntime,
  onActiveConversationModeChange,
  onActiveEffortChange,
  onActiveModelChange,
  onAgentUserMessageSent,
  onFocusIdeationSession,
  onOpenPublishPane,
  onPreloadArtifacts,
  onPublishWorkspace,
  onRenameConversation,
  onSelectArtifact,
  onToggleArtifacts,
  onSelectChatFocus,
  publishShortcutLabel,
  publishingConversationId,
  selectedConversationId,
  setTerminalChatDockElement,
  switchingConversationModeId,
  terminalUnavailableReason,
}: AgentsActiveConversationPanelProps) {
  const focusedChatSessionId = getFocusedChatSessionId(chatFocus);
  const { registry: modelRegistry } = useAgentModels();
  const panelIdeationSessionId =
    focusedChatSessionId ??
    (activeConversation.contextType === "ideation" ? activeConversation.contextId : undefined);
  const isFocusedChildChat = chatFocus.type !== "workspace";
  const showWorkspaceStatus =
    Boolean(activeWorkspace) && !isFocusedChildChat;
  const panelStoreKeyOverride = useMemo(() => {
    if (focusedChatSessionId) {
      return buildStoreKey("ideation", focusedChatSessionId);
    }
    return getAgentConversationStoreKey(activeConversation);
  }, [activeConversation, focusedChatSessionId]);

  // Every chat now renders the rich composer, which hosts the chat focus
  // pill — so the header bar should never duplicate the picker. Keep the
  // bar only when there's a workspace status pill to surface.
  const showFocusBar = showWorkspaceStatus;
  const focusBarOptions: AgentsChatFocusSwitchOption[] = [];
  const composerChatFocus = useMemo<ChatFocusFieldConfig | undefined>(() => {
    if (chatFocusOptions.length <= 1) return undefined;
    const focusToneStyles: Record<
      "accent" | "warning",
      { color: string; background: string; border: string }
    > = {
      accent: {
        color: "var(--accent-primary)",
        background: "var(--accent-muted)",
        border: "var(--accent-border)",
      },
      warning: {
        color: "var(--status-warning)",
        background: "var(--status-warning-muted)",
        border: "var(--status-warning-border)",
      },
    };
    return {
      value: chatFocus.type,
      onValueChange: (id) => onSelectChatFocus(id as AgentsChatFocusType),
      options: chatFocusOptions.map((option) => {
        const tone = option.tone ? focusToneStyles[option.tone] : null;
        const icon =
          option.type === "workspace"
            ? MessageSquare
            : option.tone === "accent"
            ? Lightbulb
            : option.tone === "warning"
            ? ShieldCheck
            : undefined;
        return {
          id: option.type,
          label: option.label,
          ...(option.description !== undefined ? { description: option.description } : {}),
          ...(icon ? { icon } : {}),
          ...(tone
            ? {
                toneColor: tone.color,
                toneBackground: tone.background,
                toneBorder: tone.border,
              }
            : {}),
        };
      }),
      testId: "agents-composer-chat-focus",
    };
  }, [chatFocus.type, chatFocusOptions, onSelectChatFocus]);
  const workspaceModelOptions = useMemo(
    () => agentModelOptions(normalizedActiveRuntime.provider, modelRegistry),
    [modelRegistry, normalizedActiveRuntime.provider]
  );
  const workspaceEffortOptions = useMemo(
    () =>
      agentEffortOptions(
        normalizedActiveRuntime.provider,
        normalizedActiveRuntime.modelId,
        modelRegistry
      ),
    [
      modelRegistry,
      normalizedActiveRuntime.modelId,
      normalizedActiveRuntime.provider,
    ]
  );

  return (
    <div className="flex-1 min-w-0 h-full flex flex-col">
      <div className="min-h-0 flex-1">
        <IntegratedChatPanel
          key={`${selectedConversationId}:${chatFocus.type}:${focusedChatSessionId ?? "workspace"}`}
          projectId={activeProjectId}
          {...(panelIdeationSessionId
            ? { ideationSessionId: panelIdeationSessionId }
            : {})}
          {...(!isFocusedChildChat
            ? { conversationIdOverride: selectedConversationId }
            : {})}
          selectedTaskIdOverride={null}
          storeContextKeyOverride={panelStoreKeyOverride}
          {...(!isFocusedChildChat && activeConversation.contextType === "project"
            ? { agentProcessContextIdOverride: selectedConversationId }
            : {})}
          {...(!isFocusedChildChat
            ? {
                sendOptions: {
                  conversationId: selectedConversationId,
                  providerHarness: normalizedActiveRuntime.provider,
                  modelId: normalizedActiveRuntime.modelId,
                  logicalEffort: normalizedActiveRuntime.effort,
                },
              }
            : {})}
          onUserMessageSent={onAgentUserMessageSent}
          onChildSessionNavigate={onFocusIdeationSession}
          hideHeaderSessionControls
          hideSessionToolbar
          surfaceBackground="transparent"
          contentWidthClassName={AGENTS_CHAT_CONTENT_WIDTH_CLASS}
          {...{
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
                  placeholder={
                    isFocusedChildChat
                      ? "Send a message..."
                      : "Ask the agent to plan, build, debug, or review something"
                  }
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
                          // Workspace conversation owns mode; child chats
                          // inherit and display it read-only.
                          disabled:
                            isFocusedChildChat ||
                            activeConversationModeLocked ||
                            composerProps.agentStatus !== "idle" ||
                            switchingConversationModeId === selectedConversationId,
                        },
                      }
                    : {})}
                  {...(composerChatFocus ? { chatFocus: composerChatFocus } : {})}
                  project={{
                    value: activeProjectId,
                    onValueChange: () => undefined,
                    options: activeProjectOptions,
                    placeholder: "Current project",
                    disabled: true,
                  }}
                  {...(() => {
                    if (!isFocusedChildChat) {
                      return {
                        provider: {
                          value: normalizedActiveRuntime.provider,
                          onValueChange: () => undefined,
                          options: AGENT_PROVIDER_OPTIONS,
                          disabled: true,
                        },
                        model: {
                          value: normalizedActiveRuntime.modelId,
                          onValueChange: onActiveModelChange,
                          options: workspaceModelOptions,
                          allowCustomValue: true,
                          customPlaceholder: "Custom model ID",
                        },
                        effort: {
                          value: normalizedActiveRuntime.effort,
                          onValueChange: onActiveEffortChange,
                          options: workspaceEffortOptions,
                          testId: "agents-conversation-effort",
                        },
                      };
                    }
                    // Child chat: use the focused session's actual runtime
                    // straight from the chat panel. We never fall back to the
                    // workspace runtime here — that produced misleading
                    // mismatched displays (e.g., "claude · gpt-5.4").
                    const childProvider =
                      (composerProps.providerHarness as AgentProvider | undefined) ??
                      undefined;
                    const childModelId = composerProps.effectiveModel?.id;
                    // Fallback provider value satisfies the typed union
                    // when harness is missing; the pill self-hides when
                    // both labels resolve empty (see ComposerRuntimePill).
                    const fallbackProvider: AgentProvider = "codex";
                    return {
                      provider: {
                        value: childProvider ?? fallbackProvider,
                        onValueChange: () => undefined,
                        options: childProvider
                          ? AGENT_PROVIDER_OPTIONS
                          : [],
                        disabled: true,
                      },
                      model: {
                        value: childModelId ?? "",
                        onValueChange: () => undefined,
                        options: childProvider
                          ? agentModelOptions(childProvider, modelRegistry)
                          : [],
                        disabled: true,
                      },
                      effort: {
                        value: "",
                        onValueChange: () => undefined,
                        options: [],
                        disabled: true,
                      },
                    };
                  })()}
                />
                <div className="mt-2 flex w-full flex-wrap items-center justify-between gap-2 px-2">
                  <AgentComposerProjectLine
                    value={activeProjectId}
                    onValueChange={() => undefined}
                    options={activeProjectOptions}
                    placeholder="Current project"
                    disabled
                  />
                  <AgentConversationBaseLine workspace={activeWorkspace} />
                </div>
              </>
            ),
          }}
          {...(!isFocusedChildChat && activeConversation.contextType === "project" && attachedIdeationSessionId
            ? { additionalQuestionSessionIds: [attachedIdeationSessionId] }
            : {})}
          headerContent={
            <AgentsChatHeaderController
              conversation={activeConversation}
              workspace={isFocusedChildChat ? null : activeWorkspace}
              chatFocus={chatFocus}
              availableArtifactTabs={availableArtifactTabs}
              modelDisplay={{
                id: normalizedActiveRuntime.modelId,
                label: normalizedActiveRuntime.modelId,
              }}
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
          {...(showFocusBar
            ? {
                headerSubContent: (
                  <AgentsChatFocusBar
                    activeType={chatFocus.type}
                    options={focusBarOptions}
                    workspace={showWorkspaceStatus ? activeWorkspace : null}
                    onSelectFocus={onSelectChatFocus}
                  />
                ),
              }
            : {})}
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
