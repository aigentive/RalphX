import { useMemo, type ReactNode } from "react";
import { StatusActivityBadge } from "./StatusActivityBadge";
import type { AgentType, StatusActivityBadgeProps } from "./StatusActivityBadge";
import type { ContextType, ModelDisplay, ChatConversation } from "@/types/chat-conversation";
import type { AgentStatus } from "@/stores/chatStore";
import type { ChatMessageResponse } from "@/api/chat";
import {
  formatProviderHarnessLabel,
  formatProviderEvidenceTooltip,
  getProviderHarnessBadgeStyle,
} from "./provider-harness";
import { ModelChip } from "./ModelChip";
import { EffortChip } from "./EffortChip";
import { ConversationStatsPopover } from "./ConversationStatsPopover";
import { useConversationStats } from "@/hooks/useConversationStats";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";

export interface ChatSessionToolbarProps {
  backAction?: {
    label: string;
    icon?: ReactNode;
    onClick: () => void;
  };
  isAgentActive: StatusActivityBadgeProps["isAgentActive"];
  agentType: AgentType;
  contextType: ContextType;
  contextId: string | null;
  hasActivity?: boolean;
  agentStatus?: AgentStatus;
  storeKey?: string;
  modelDisplay?: ModelDisplay;
  conversationId?: string | null;
  providerHarness?: string | null;
  providerSessionId?: string | null;
  upstreamProvider?: string | null;
  providerProfile?: string | null;
  fallbackConversation?: ChatConversation | null | undefined;
  fallbackMessages?: ChatMessageResponse[] | null | undefined;
  /**
   * When true, skip rendering the provider-context chips (harness badge,
   * ModelChip, EffortChip, stats popover) — used when those chips are
   * rendered elsewhere (e.g., inline in the Conversation header).
   */
  hideProviderContext?: boolean;
}

export function ChatSessionToolbar({
  backAction,
  isAgentActive,
  agentType,
  contextType,
  contextId,
  hasActivity,
  agentStatus,
  storeKey,
  modelDisplay,
  conversationId,
  providerHarness,
  providerSessionId,
  upstreamProvider,
  providerProfile,
  fallbackConversation,
  fallbackMessages,
  hideProviderContext = false,
}: ChatSessionToolbarProps) {
  const { data: featureFlags } = useFeatureFlags();
  const statsFallbackConversation = useMemo(() => {
    if (fallbackConversation) {
      return fallbackConversation;
    }

    if (!conversationId || !contextId) {
      return null;
    }

    const lastMessageAt =
      fallbackMessages && fallbackMessages.length > 0
        ? (fallbackMessages[fallbackMessages.length - 1]?.createdAt ?? null)
        : null;
    const timestamp = lastMessageAt ?? new Date().toISOString();

    return {
      id: conversationId,
      contextType,
      contextId,
      claudeSessionId:
        providerHarness === "claude" ? (providerSessionId ?? null) : null,
      providerSessionId: providerSessionId ?? null,
      providerHarness: providerHarness ?? null,
      upstreamProvider: upstreamProvider ?? null,
      providerProfile: providerProfile ?? null,
      title: null,
      messageCount: fallbackMessages?.length ?? 0,
      lastMessageAt,
      createdAt: timestamp,
      updatedAt: timestamp,
    };
  }, [
    fallbackConversation,
    conversationId,
    contextType,
    contextId,
    providerHarness,
    providerSessionId,
    upstreamProvider,
    providerProfile,
    fallbackMessages,
  ]);

  const statsQuery = useConversationStats(conversationId ?? null, {
    fallbackConversation: statsFallbackConversation,
    fallbackMessages,
  });
  const stats = statsQuery.data;
  const effortKey = stats?.byEffort[0]?.key ?? null;
  const harnessLabel = formatProviderHarnessLabel(providerHarness);
  const harnessStyle = getProviderHarnessBadgeStyle(providerHarness);
  const providerTooltip = formatProviderEvidenceTooltip({
    providerHarness,
    providerSessionId,
    upstreamProvider,
    providerProfile,
  });
  const showStats = Boolean(stats);
  const showProviderContext =
    !hideProviderContext &&
    (harnessLabel !== null ||
      modelDisplay != null ||
      effortKey != null ||
      showStats);
  const showStatus =
    isAgentActive ||
    agentStatus === "waiting_for_input" ||
    (hasActivity === true && featureFlags.activityPage === true);

  if (!backAction && !showProviderContext && !showStatus) {
    return null;
  }

  return (
    <div
      className="px-3 py-1.5 shrink-0"
      style={{ borderBottom: "1px solid var(--border-subtle)" }}
    >
      <div
        className="flex min-w-0 items-center gap-2"
        data-testid="chat-session-toolbar-row"
      >
        {backAction && (
          <button
            data-testid="back-to-plan-button"
            onClick={backAction.onClick}
            className="flex shrink-0 items-center gap-1 text-xs text-text-primary/50 hover:text-text-primary/80 transition-colors"
          >
            {backAction.icon}
            <span>{backAction.label}</span>
          </button>
        )}
        <div className="flex min-w-0 flex-1 items-center justify-between gap-2">
          {showProviderContext ? (
            <div
              className="flex min-w-0 flex-1 items-center gap-2"
              data-testid="chat-session-provider-context"
            >
              {harnessLabel && (
                <span
                  className="rounded-full px-1.5 py-0.5 text-[0.5625rem] font-semibold uppercase tracking-[0.08em]"
                  style={harnessStyle}
                  title={providerTooltip ?? undefined}
                  aria-label={providerTooltip ?? harnessLabel}
                  data-testid="chat-session-provider-badge"
                >
                  {harnessLabel}
                </span>
              )}
              {modelDisplay && <ModelChip model={modelDisplay} />}
              {effortKey && <EffortChip effort={effortKey} />}
              {showStats && (
                <ConversationStatsPopover
                  conversationId={conversationId ?? null}
                  fallbackConversation={statsFallbackConversation}
                  fallbackMessages={fallbackMessages}
                  stats={stats}
                  isLoading={statsQuery.isLoading}
                  isLiveTurnActive={isAgentActive}
                />
              )}
            </div>
          ) : (
            <div className="flex-1" />
          )}
          {showStatus && (
            <div className="shrink-0">
              <StatusActivityBadge
                isAgentActive={isAgentActive}
                agentType={agentType}
                contextType={contextType}
                contextId={contextId}
                {...(hasActivity !== undefined ? { hasActivity } : {})}
                {...(agentStatus !== undefined ? { agentStatus } : {})}
                {...(storeKey !== undefined ? { storeKey } : {})}
                {...(modelDisplay !== undefined ? { modelDisplay } : {})}
                hideModelChip
                layout="inline"
              />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
