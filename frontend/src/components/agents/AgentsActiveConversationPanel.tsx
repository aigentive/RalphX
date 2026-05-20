import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { Clock, Lightbulb, MessageSquare, ShieldCheck } from "lucide-react";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
  AgentConversationWorkspaceMode,
} from "@/api/chat";
import {
  IntegratedChatPanel,
  type IntegratedChatComposerRenderProps,
} from "@/components/Chat/IntegratedChatPanel";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { formatQueuedMessageExcerpt } from "@/lib/queuedMessageExcerpt";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { selectQueuedMessages, useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
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
import { AgentsComposerWorkspaceChangesCard } from "./AgentsComposerWorkspaceChangesCard";
import { AgentsChatHeaderController } from "./AgentsChatHeaderController";
import {
  AGENT_CONVERSATION_MODE_OPTIONS,
} from "./agentConversationMode";
import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import {
  AGENT_PROVIDER_OPTIONS,
  agentEffortOptions,
  agentModelOptions,
  normalizeRuntimeSelection,
} from "./agentOptions";
import { AgentProviderSettingsButton } from "./AgentProviderSettingsButton";
import {
  buildAgentProviderAvailabilityOptions,
  getProviderAvailabilityMessage,
  supportedEffortsForProvider,
} from "./agentProviderAvailability";
import { AgentsTerminalDockHost } from "./AgentsTerminalRegion";
import { AGENTS_CHAT_MIN_WIDTH } from "./AgentsArtifactPaneRegion";
import {
  getAgentQueueHaltState,
  type AgentQueueHaltState,
} from "./agentExecutionPause";
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
  activeWorkspaceFreshness: AgentConversationWorkspaceFreshness | undefined;
  attachedIdeationSessionId: string | null;
  availableArtifactTabs: readonly IdeationArtifactTab[];
  chatFocus: AgentsChatFocus;
  chatFocusOptions: readonly AgentsChatFocusSwitchOption[];
  hasAutoOpenArtifacts: boolean;
  normalizedActiveRuntime: AgentRuntimeSelection;
  onActiveConversationModeChange: (mode: AgentConversationWorkspaceMode) => void;
  onActiveConversationModeMenuOpen: () => void;
  onActiveEffortChange: (
    effort: string,
    providerSupportedEfforts?: readonly string[] | null
  ) => void;
  onActiveModelChange: (
    modelId: string,
    providerSupportedEfforts?: readonly string[] | null
  ) => void;
  onAgentUserMessageSent: (event: {
    content: string;
    result: { conversationId: string };
  }) => void;
  onFocusIdeationSession: (sessionId: string) => void;
  onOpenPublishPane: () => void;
  onOpenPublishFile: (filePath: string, mode: DiffFilterMode) => void;
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
  activeWorkspaceFreshness,
  attachedIdeationSessionId,
  availableArtifactTabs,
  chatFocus,
  chatFocusOptions,
  hasAutoOpenArtifacts,
  normalizedActiveRuntime,
  onActiveConversationModeChange,
  onActiveConversationModeMenuOpen,
  onActiveEffortChange,
  onActiveModelChange,
  onAgentUserMessageSent,
  onFocusIdeationSession,
  onOpenPublishPane,
  onOpenPublishFile,
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
  const openModal = useUiStore((s) => s.openModal);
  const {
    providers: configuredProviders,
    isLoading: isLoadingProviderSettings,
    isPlaceholderData: isPlaceholderProviderSettings,
  } = useHarnessProviders();
  const providerSettingsReady =
    !isLoadingProviderSettings && !isPlaceholderProviderSettings;
  const providerOptions = useMemo(
    () =>
      buildAgentProviderAvailabilityOptions({
        providers: configuredProviders,
        isReady: providerSettingsReady,
      }),
    [configuredProviders, providerSettingsReady],
  );
  const [composerActivityTick, setComposerActivityTick] = useState(0);
  const [isComposerHydrationPaused, setIsComposerHydrationPaused] = useState(false);
  const markComposerActivity = useCallback(() => {
    setIsComposerHydrationPaused(true);
    setComposerActivityTick((tick) => tick + 1);
  }, []);
  useEffect(() => {
    setIsComposerHydrationPaused(false);
  }, [selectedConversationId]);
  useEffect(() => {
    if (!isComposerHydrationPaused) {
      return;
    }

    const timer = window.setTimeout(() => {
      setIsComposerHydrationPaused(false);
    }, 900);

    return () => window.clearTimeout(timer);
  }, [composerActivityTick, isComposerHydrationPaused]);
  const workspaceProviderStatusMessage = getProviderAvailabilityMessage({
    provider: normalizedActiveRuntime.provider,
    providerOptions,
    isReady: providerSettingsReady,
  });
  const workspaceProviderSupportedEfforts = useMemo(
    () =>
      supportedEffortsForProvider(
        providerOptions,
        normalizedActiveRuntime.provider
      ),
    [normalizedActiveRuntime.provider, providerOptions]
  );
  const selectableWorkspaceRuntime = useMemo(
    () =>
      normalizeRuntimeSelection(
        normalizedActiveRuntime,
        modelRegistry,
        workspaceProviderSupportedEfforts
      ),
    [modelRegistry, normalizedActiveRuntime, workspaceProviderSupportedEfforts]
  );
  const openProviderSettings = useCallback(() => {
    openModal("settings", { section: "providers" });
  }, [openModal]);
  const panelIdeationSessionId =
    focusedChatSessionId ??
    (activeConversation.contextType === "ideation" ? activeConversation.contextId : undefined);
  const isFocusedChildChat = chatFocus.type !== "workspace";
  const panelStoreKeyOverride = useMemo(() => {
    if (focusedChatSessionId) {
      return buildStoreKey("ideation", focusedChatSessionId);
    }
    return getAgentConversationStoreKey(activeConversation);
  }, [activeConversation, focusedChatSessionId]);
  const queuedMessagesSelector = useMemo(
    () => selectQueuedMessages(panelStoreKeyOverride),
    [panelStoreKeyOverride]
  );
  const queuedMessages = useChatStore(queuedMessagesSelector);
  const executionHaltState = useUiStore((s) =>
    getAgentQueueHaltState(s.executionStatus)
  );
  const queuedInitialPrompt = queuedMessages[0]?.content ?? null;
  const emptyState = useMemo(
    () =>
      executionHaltState && queuedInitialPrompt ? (
        <AgentsPausedQueuedEmptyState
          haltState={executionHaltState}
          prompt={queuedInitialPrompt}
        />
      ) : (
        <div />
      ),
    [executionHaltState, queuedInitialPrompt]
  );

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
    () => agentModelOptions(selectableWorkspaceRuntime.provider, modelRegistry),
    [modelRegistry, selectableWorkspaceRuntime.provider]
  );
  const workspaceEffortOptions = useMemo(
    () =>
      agentEffortOptions(
        selectableWorkspaceRuntime.provider,
        selectableWorkspaceRuntime.modelId,
        modelRegistry,
        workspaceProviderSupportedEfforts
      ),
    [
      modelRegistry,
      selectableWorkspaceRuntime.modelId,
      selectableWorkspaceRuntime.provider,
      workspaceProviderSupportedEfforts,
    ]
  );
  const modeOptions = useMemo(() => {
    if (!activeConversationModeLocked) {
      return AGENT_CONVERSATION_MODE_OPTIONS;
    }
    const lockReason =
      activeWorkspace?.modeSwitchLockReason ??
      "Active ideation or execution state owns this workspace.";
    return AGENT_CONVERSATION_MODE_OPTIONS.map((option) =>
      option.id === activeConversationMode || option.id === "ideation"
        ? option
        : {
            ...option,
            disabled: true,
            disabledReason: lockReason,
          },
    );
  }, [
    activeConversationMode,
    activeConversationModeLocked,
    activeWorkspace?.modeSwitchLockReason,
  ]);

  return (
    <div
      className="flex-1 h-full flex flex-col"
      style={{ minWidth: AGENTS_CHAT_MIN_WIDTH }}
      data-testid="agents-active-conversation-panel"
    >
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
                <AgentsComposerWorkspaceChangesCard
                  conversationId={selectedConversationId}
                  projectId={activeProjectId}
                  workspace={activeWorkspace}
                  isFocusedChildChat={isFocusedChildChat}
                  isAgentGenerating={composerProps.agentStatus === "generating"}
                  pauseHydration={isComposerHydrationPaused}
                  onOpenFile={onOpenPublishFile}
                  onPreloadPublishPane={onPreloadArtifacts}
                />
                <AgentComposerSurface
                  dataTestId="agents-conversation-composer"
                  actionTestId="agents-conversation-submit"
                  onSend={composerProps.onSend}
                  onStop={composerProps.onStop}
                  agentStatus={composerProps.agentStatus}
                  isSubmitting={composerProps.isSending}
                  isReadOnly={composerProps.isReadOnly}
                  autoFocus={composerProps.autoFocus}
                  conversationId={selectedConversationId}
                  placeholder={
                    isFocusedChildChat
                      ? "Send a message..."
                      : "Ask the agent to plan, build, debug, or review something"
                  }
                  onFocusChange={(focused) => {
                    if (focused) {
                      markComposerActivity();
                    }
                  }}
                  sendDisabledReason={
                    !isFocusedChildChat ? workspaceProviderStatusMessage : null
                  }
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
                        onChange: (value: string) => {
                          markComposerActivity();
                          composerProps.onChange?.(value);
                        },
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
                          onOpen: onActiveConversationModeMenuOpen,
                          onValueChange: (value: string) =>
                            onActiveConversationModeChange(
                              value as AgentConversationWorkspaceMode,
                            ),
                          options: modeOptions,
                          // Workspace conversation owns mode; child chats
                          // inherit and display it read-only.
                          disabled:
                            isFocusedChildChat ||
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
                          options: providerOptions.length > 0 ? providerOptions : AGENT_PROVIDER_OPTIONS,
                          disabled: true,
                          footerAction: (
                            <AgentProviderSettingsButton
                              onClick={openProviderSettings}
                              testId="agents-conversation-provider-settings"
                            />
                          ),
                        },
                        model: {
                          value: selectableWorkspaceRuntime.modelId,
                          onValueChange: (modelId) =>
                            onActiveModelChange(
                              modelId,
                              workspaceProviderSupportedEfforts
                            ),
                          options: workspaceModelOptions,
                          disabled: Boolean(workspaceProviderStatusMessage),
                          allowCustomValue: true,
                          customPlaceholder: "Custom model ID",
                        },
                        effort: {
                          value: selectableWorkspaceRuntime.effort,
                          onValueChange: (effort) =>
                            onActiveEffortChange(
                              effort,
                              workspaceProviderSupportedEfforts
                            ),
                          options: workspaceEffortOptions,
                          disabled: Boolean(workspaceProviderStatusMessage),
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
                {!isFocusedChildChat && workspaceProviderStatusMessage && (
                  <div
                    className="mx-2 mt-2 flex flex-wrap items-center justify-between gap-2 rounded-md border px-3 py-2 text-[0.8125rem]"
                    style={{
                      color: "var(--text-secondary)",
                      background: "var(--bg-surface)",
                      borderColor: "var(--border-subtle)",
                    }}
                    data-testid="agents-conversation-provider-status"
                  >
                    <span>{workspaceProviderStatusMessage}</span>
                    <button
                      type="button"
                      className="rounded-md px-2 py-1 text-[0.75rem] font-medium"
                      style={{
                        color: "var(--accent-primary)",
                        background: "var(--accent-muted)",
                      }}
                      onClick={openProviderSettings}
                      data-testid="agents-conversation-provider-status-settings"
                    >
                      Open Settings
                    </button>
                  </div>
                )}
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
                    {...(activeWorkspaceFreshness
                      ? { freshness: activeWorkspaceFreshness }
                      : {})}
                  />
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
              showTitle={false}
            />
          }
          emptyState={emptyState}
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

function AgentsPausedQueuedEmptyState({
  haltState,
  prompt,
}: {
  haltState: NonNullable<AgentQueueHaltState>;
  prompt: string;
}) {
  const title =
    haltState === "stopped" ? "Execution is stopped" : "Execution is paused";
  const detail =
    haltState === "stopped"
      ? "This prompt will start when execution starts."
      : "This prompt will start when execution resumes.";
  const promptExcerpt = formatQueuedMessageExcerpt(prompt);

  return (
    <div
      data-testid="agents-paused-queued-empty-state"
      className="flex h-full w-full items-center justify-center p-6"
    >
      <div className="w-full max-w-[360px] text-center">
        <div
          className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-md border"
          style={{
            backgroundColor: "var(--status-warning-muted)",
            borderColor: "var(--status-warning-border)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--status-warning)",
          }}
        >
          <Clock className="h-5 w-5" />
        </div>
        <h3
          className="text-base font-semibold tracking-tight"
          style={{ color: "var(--text-primary)" }}
        >
          {title}
        </h3>
        <p
          className="mt-2 text-sm leading-relaxed"
          style={{ color: "var(--text-secondary)" }}
        >
          {detail}
        </p>
        <div
          className="mt-5 rounded-md border px-3 py-2.5 text-left"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          <p
            className="text-[11px] font-medium uppercase tracking-[0.12em]"
            style={{ color: "var(--text-muted)" }}
          >
            Queued prompt
          </p>
          <p
            data-testid="agents-paused-queued-prompt"
            className="mt-1 line-clamp-4 text-sm leading-relaxed"
            style={{ color: "var(--text-secondary)" }}
          >
            {promptExcerpt}
          </p>
        </div>
      </div>
    </div>
  );
}
