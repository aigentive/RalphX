import { useMemo } from "react";
import type { ContextType, ModelDisplay, ChatConversation } from "@/types/chat-conversation";
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

export interface ChatSessionChipsProps {
  contextType: ContextType;
  contextId: string | null;
  isAgentActive: boolean;
  modelDisplay?: ModelDisplay;
  conversationId?: string | null;
  providerHarness?: string | null;
  providerSessionId?: string | null;
  upstreamProvider?: string | null;
  providerProfile?: string | null;
  fallbackConversation?: ChatConversation | null | undefined;
  fallbackMessages?: ChatMessageResponse[] | null | undefined;
  showProviderModel?: boolean;
  showStats?: boolean;
}

/**
 * Inline, chrome-less render of the provider-context chips
 * (harness badge + ModelChip + EffortChip + ConversationStatsPopover).
 * Use inside a host row (e.g., the Conversation header) instead of the
 * standalone `ChatSessionToolbar` row.
 */
export function ChatSessionChips({
  contextType,
  contextId,
  isAgentActive,
  modelDisplay,
  conversationId,
  providerHarness,
  providerSessionId,
  upstreamProvider,
  providerProfile,
  fallbackConversation,
  fallbackMessages,
  showProviderModel = true,
  showStats = true,
}: ChatSessionChipsProps) {
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

  const statsQuery = useConversationStats(showStats ? conversationId ?? null : null, {
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
  const showStatsChip = showStats && Boolean(stats);
  const hasAnyChip =
    (showProviderModel &&
      (harnessLabel !== null || modelDisplay != null || effortKey != null)) ||
    showStatsChip;

  if (!hasAnyChip) {
    return null;
  }

  return (
    <div
      className="flex min-w-0 items-center gap-2"
      data-testid="chat-session-chips"
    >
      {showProviderModel && harnessLabel && (
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
      {showProviderModel && modelDisplay && <ModelChip model={modelDisplay} />}
      {showProviderModel && effortKey && <EffortChip effort={effortKey} />}
      {showStatsChip && (
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
  );
}
