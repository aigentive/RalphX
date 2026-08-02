/**
 * ChatMessageList - Virtualized message list for chat panels
 *
 * Wraps react-virtuoso with chat-specific rendering:
 * - Auto-scroll to bottom
 * - Failed run banner header
 * - Worker executing indicator
 * - Streaming tool calls / typing indicator footer
 */

import React, { forwardRef, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useImperativeHandle } from "react";
import { Virtuoso, type ListRange, type ScrollerProps, type VirtuosoHandle } from "react-virtuoso";
import { MessageItem, MessageMeta } from "./MessageItem";
import { parseComposerReferencesFromMetadata } from "./MessageReferences.parse";
import { HookEventMessage } from "./HookEventMessage";
import {
  ConversationTranscriptPlaceholders,
  TypingIndicator,
  FailedRunBanner,
} from "./IntegratedChatPanel.components";
import { TextBubble } from "./TextBubble";
import { ToolCallIndicator } from "./ToolCallIndicator";
import type { ToolCall } from "./ToolCallIndicator";
import type { StreamingTask, StreamingContentBlock } from "@/types/streaming-task";
import type { ContentBlockItem } from "./MessageItem";
import type { HookEvent, HookStartedEvent } from "@/types/hook-event";
import { isDiffToolCall, isTaskToolCall } from "./DiffToolCallView.utils";
import { DiffToolCallView } from "./DiffToolCallView";
import { TaskSubagentCard } from "./TaskSubagentCard";
import { shouldUseWebkitSafeScrollBehavior } from "@/lib/platform-quirks";
import { logger } from "@/lib/logger";
import { useMessageAttachments } from "@/hooks/useMessageAttachments";
import { useRunAttributions } from "@/hooks/useRunAttributions";
import type { AgentRunAttribution } from "@/api/agent-runs";
import { ChevronDown } from "lucide-react";
import type { MessageAttachment } from "./MessageAttachments";
import { ToolCallStoreKeyContext } from "./tool-widgets/ToolCallStoreKeyContext";
import { shouldHideCompletedProjectOrchestrationToolCall } from "./tool-widgets/ProjectOrchestrationWidget.utils";
import { isProviderRole } from "@/lib/chat/provider-role";
import { selectActiveAgentRunId, useChatStore } from "@/stores/chatStore";
import { cn } from "@/lib/utils";
import { isTranscriptRootReadyForReveal } from "./ChatMessageList.readiness";
import { VISUAL_BOTTOM_EPSILON_PX } from "./ChatMessageList.scroll";
import {
  createChatScrollController,
  type ChatScrollController,
  type ChatScrollControllerDeps,
} from "./scroll/controller";
import {
  buildLiveTranscriptRows,
  isLiveThinkingGroupKey,
  liveThinkingGroupKey,
  liveToolGroupKey,
  synchronizeThinkingGroupExpansion,
  type LiveTranscriptRow,
  type StreamingToolUseBlock,
  type ThinkingGroupIntent,
} from "./ChatMessageList.liveRows";
import type { AgentRun } from "@/types/chat-conversation";
import { ToolActivityGroupToggle } from "./ToolActivityGroupToggle";
import { ThinkingGroupToggle } from "./ThinkingGroupToggle";
import { RunAttributionWidget } from "./RunAttributionWidget";
import { ThinkingWidget } from "./tool-widgets/ThinkingWidget";
import {
  summarizeToolActivity,
  type ToolActivitySummary,
  type ToolActivityTask,
} from "./tool-activity-summary";
import {
  delegationJobIdForToolCall,
  projectDelegationTimelineMessages,
  persistedDelegationJobIds,
  persistedTimelineToolCall,
} from "./delegation-timeline";

// ============================================================================
// Constants
// ============================================================================

const INITIAL_TRANSCRIPT_PAINT_MAX_FRAMES = 240;
const INITIAL_TRANSCRIPT_PAINT_MAX_MS = 2_500;

/** Shared styles for content containers to handle long text */
const contentContainerStyle: React.CSSProperties = {
  maxWidth: "100%",
  overflowWrap: "break-word",
  wordBreak: "break-word",
  overflowAnchor: "none",
};

type ChatVirtuosoScrollerProps = ScrollerProps & {
  className?: string;
  context?: unknown;
};

const ChatVirtuosoScroller = forwardRef<HTMLDivElement, ChatVirtuosoScrollerProps>(
  function ChatVirtuosoScroller({ context: _context, style, className, ...props }, ref) {
    return (
      <div
        {...props}
        ref={ref}
        tabIndex={0}
        data-chat-virtuoso-scroller="true"
        className={cn(
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)]",
          className,
        )}
        style={{
          ...style,
          overflowAnchor: "none",
          overscrollBehavior: "none",
        }}
      />
    );
  },
);

function isNestedScrollableWheelTarget(
  target: EventTarget | null,
  root: HTMLElement,
): boolean {
  if (!(target instanceof HTMLElement) || target === root) {
    return false;
  }

  let element: HTMLElement | null = target;
  while (element && element !== root) {
    const { overflowY } = window.getComputedStyle(element);
    const canScrollY =
      overflowY === "auto" ||
      overflowY === "scroll" ||
      overflowY === "overlay";
    if (
      canScrollY &&
      element.scrollHeight > element.clientHeight + VISUAL_BOTTOM_EPSILON_PX
    ) {
      return true;
    }
    element = element.parentElement;
  }

  return false;
}

function isEditableOrInteractiveTarget(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && target.closest(
    'input, textarea, select, [contenteditable], [role="textbox"]',
  ) !== null;
}

function ContentShell({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string | undefined;
}) {
  return (
    <div
      className={cn("w-full", className ? ["mx-auto", className] : undefined)}
      data-testid="chat-message-content-shell"
    >
      {children}
    </div>
  );
}

function ScrollToBottomControl({
  visible,
  onClick,
  onWheel,
}: {
  visible: boolean;
  onClick: () => void;
  onWheel: React.WheelEventHandler<HTMLButtonElement>;
}) {
  return (
    <div
      data-testid="chat-scroll-to-bottom-control"
      aria-hidden={!visible}
      className={cn(
        "absolute bottom-4 left-0 right-0 z-10 flex justify-center pointer-events-none",
        visible ? "opacity-100" : "opacity-0",
      )}
      style={{
        contain: "layout paint style",
      }}
    >
      <button
        type="button"
        data-testid="chat-scroll-to-bottom-button"
        onClick={onClick}
        onWheel={onWheel}
        disabled={!visible}
        tabIndex={visible ? 0 : -1}
        className={cn(
          "inline-flex h-8 items-center gap-1.5 rounded-md border px-3 text-xs font-medium",
          "bg-[color-mix(in_srgb,var(--bg-surface)_72%,var(--bg-base))]",
          "border-[color-mix(in_srgb,var(--border-subtle)_45%,var(--text-muted))]",
          "text-[var(--text-primary)] hover:bg-[color-mix(in_srgb,var(--bg-surface)_58%,var(--bg-base))]",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)]",
          visible ? "pointer-events-auto cursor-pointer" : "pointer-events-none cursor-default",
        )}
      >
        <span>Scroll to bottom</span>
        <ChevronDown className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
      </button>
    </div>
  );
}

/** Stable empty arrays — avoids new refs on each render when props are omitted */
const EMPTY_HOOK_EVENTS: HookEvent[] = [];
const EMPTY_ACTIVE_HOOKS: HookStartedEvent[] = [];

// ============================================================================
// Types
// ============================================================================

export interface ChatMessageData {
  id: string;
  role: string;
  content: string;
  createdAt: string;
  parentMessageId?: string | null;
  toolCalls?: ToolCall[] | null;
  contentBlocks?: ContentBlockItem[] | null;
  attachments?: MessageAttachment[];
  sender?: string | null;
  metadata?: string | null;
  providerHarness?: string | null;
  providerSessionId?: string | null;
  upstreamProvider?: string | null;
  providerProfile?: string | null;
  logicalModel?: string | null;
  effectiveModelId?: string | null;
  logicalEffort?: string | null;
  effectiveEffort?: string | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
  cacheCreationTokens?: number | null;
  cacheReadTokens?: number | null;
  estimatedUsd?: number | null;
  timelineSequence?: number | null;
  runId?: string | null;
  finalizedAt?: string | null;
}

type ToolCallGroupMarker = {
  key: string;
  count: number;
  summary: ToolActivitySummary;
  promoted: boolean;
  position: "toggle" | "covered";
};


type TimelineMessageItem = {
  kind: "message";
  data: ChatMessageData;
  sortTime: number;
  toolCallGroup?: ToolCallGroupMarker;
};

/** Discriminated union for timeline items when hook events are interleaved */
type TimelineItem =
  | TimelineMessageItem
  | { kind: "hook"; data: HookEvent | HookStartedEvent; sortTime: number }
  | { kind: "streaming_row"; data: LiveTranscriptRow; sortTime: number }
  | { kind: "streaming"; sortTime: number };

function parseMessageMetadata(metadata: string | null | undefined): Record<string, unknown> | null {
  if (!metadata) return null;
  try {
    return JSON.parse(metadata) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function isPersistedTimelineToolCallMessage(message: ChatMessageData): boolean {
  if (!isProviderRole(message.role) || message.timelineSequence == null) {
    return false;
  }
  const blocks = message.contentBlocks;
  return blocks?.length === 1 && blocks[0]?.type === "tool_use";
}

function sameToolGroupSurface(left: ChatMessageData, right: ChatMessageData): boolean {
  return left.role === right.role
    && (left.sender ?? null) === (right.sender ?? null)
    && (left.providerHarness ?? null) === (right.providerHarness ?? null)
    && (left.providerSessionId ?? null) === (right.providerSessionId ?? null)
    && (left.upstreamProvider ?? null) === (right.upstreamProvider ?? null)
    && (left.providerProfile ?? null) === (right.providerProfile ?? null);
}

function canContinueToolCallGroup(
  first: ChatMessageData,
  previous: ChatMessageData,
  next: ChatMessageData,
): boolean {
  if (!isPersistedTimelineToolCallMessage(next) || !sameToolGroupSurface(first, next)) {
    return false;
  }
  if (first.parentMessageId || next.parentMessageId) {
    if (!first.parentMessageId || first.parentMessageId !== next.parentMessageId) {
      return false;
    }
  }
  if (
    previous.timelineSequence != null
    && next.timelineSequence != null
    && next.timelineSequence !== previous.timelineSequence + 1
  ) {
    return false;
  }
  return true;
}

function liveTranscriptRowSortTime(
  _row: LiveTranscriptRow,
  rowIndex: number,
  rowCount: number,
): number {
  return Number.MAX_SAFE_INTEGER - rowCount + rowIndex - 1;
}

function collectToolCallGroupRun(
  messages: ChatMessageData[],
  startIndex: number,
): ChatMessageData[] | null {
  const first = messages[startIndex];
  if (!first || !isPersistedTimelineToolCallMessage(first)) {
    return null;
  }

  const group = [first];
  let previous = first;
  for (let index = startIndex + 1; index < messages.length; index += 1) {
    const next = messages[index];
    if (!next || !canContinueToolCallGroup(first, previous, next)) {
      break;
    }
    group.push(next);
    previous = next;
  }

  return group.length >= 1 ? group : null;
}

function toolCallGroupKey(messages: ChatMessageData[]): string {
  const first = messages[0];
  const last = messages[messages.length - 1];
  if (!first || !last) {
    return "tool-call-group:empty";
  }
  const firstSequence = first.timelineSequence ?? first.id;
  const lastSequence = last.timelineSequence ?? last.id;
  return [
    "tool-call-group",
    first.parentMessageId ?? first.id,
    firstSequence,
    lastSequence,
    messages.length,
  ].join(":");
}

function isCollapsedToolCallGroupCoveredItem(
  item: TimelineItem,
  expandedToolGroupKeys: Set<string>,
): boolean {
  return item.kind === "message"
    && item.toolCallGroup?.position === "covered"
    && !item.toolCallGroup.promoted
    && !expandedToolGroupKeys.has(item.toolCallGroup.key);
}

function isVisibleTimelineItem(
  item: TimelineItem,
  expandedToolGroupKeys: Set<string>,
): boolean {
  return !isCollapsedToolCallGroupCoveredItem(item, expandedToolGroupKeys);
}

interface RunAttributionTiming {
  runId: string;
  startedAt: string;
  completedAt: string | null;
  launchRole: string | null;
}

interface ResolvedRunAttributionTiming extends RunAttributionTiming {
  attribution: AgentRunAttribution | null;
}

/**
 * One pass over the transcript: for every settled run, compute its timing span
 * and anchor the worked-widget on the run's last provider message. The active
 * run is excluded so a live run never shows a premature "worked" widget.
 */
function buildRunAttributionTimingByMessageId(
  messages: readonly ChatMessageData[],
  activeRunId: string | null | undefined,
): Map<string, RunAttributionTiming> {
  const timingByRun = new Map<
    string,
    { startedAt: number; completedAt: number; lastMessageId: string }
  >();
  for (const message of messages) {
    if (!message.runId || !isProviderRole(message.role)) continue;
    if (activeRunId != null && message.runId === activeRunId) continue;
    const created = Date.parse(message.createdAt);
    const finalized = Date.parse(message.finalizedAt ?? message.createdAt);
    const existing = timingByRun.get(message.runId);
    if (!existing) {
      timingByRun.set(message.runId, {
        startedAt: created,
        completedAt: finalized,
        lastMessageId: message.id,
      });
      continue;
    }
    if (Number.isFinite(created) && !(created >= existing.startedAt)) {
      existing.startedAt = created;
    }
    if (Number.isFinite(finalized) && !(finalized <= existing.completedAt)) {
      existing.completedAt = finalized;
    }
    existing.lastMessageId = message.id;
  }
  const byMessageId = new Map<string, RunAttributionTiming>();
  for (const [runId, timing] of timingByRun) {
    if (!Number.isFinite(timing.startedAt)) continue;
    byMessageId.set(timing.lastMessageId, {
      runId,
      startedAt: new Date(timing.startedAt).toISOString(),
      completedAt: Number.isFinite(timing.completedAt)
        ? new Date(timing.completedAt).toISOString()
        : null,
      launchRole: null,
    });
  }
  return byMessageId;
}

function senderGroupPart(value: string | null | undefined) {
  const trimmed = value?.trim();
  return trimmed && trimmed.length > 0 ? trimmed : "";
}

export type PersonaAttributedRun = Pick<
  AgentRun,
  | "id"
  | "personaId"
  | "personaSlug"
  | "personaVersion"
  | "personaInjected"
  | "personaSkippedReason"
>;

function personaRunBadgeProps(
  agentRun: PersonaAttributedRun | null | undefined,
  enabled: boolean,
  runId: string | null | undefined,
) {
  if (!enabled || !agentRun || runId !== agentRun.id) {
    return {};
  }
  return {
    agentPersonasEnabled: true,
    personaId: agentRun.personaId ?? null,
    personaSlug: agentRun.personaSlug ?? null,
    personaVersion: agentRun.personaVersion ?? null,
    personaInjected: agentRun.personaInjected ?? null,
    personaSkippedReason: agentRun.personaSkippedReason ?? null,
  };
}

function attributedRunForId(
  activeRun: PersonaAttributedRun | null | undefined,
  historicalRuns: readonly PersonaAttributedRun[],
  runId: string | null | undefined,
) {
  if (!runId) return null;
  return activeRun?.id === runId
    ? activeRun
    : (historicalRuns.find((run) => run.id === runId) ?? null);
}

function assistantSenderGroupKeyForMessage(message: ChatMessageData): string | null {
  if (!isProviderRole(message.role)) {
    return null;
  }
  return [
    "assistant",
    senderGroupPart(message.sender),
    senderGroupPart(message.providerHarness),
    senderGroupPart(message.providerSessionId),
    senderGroupPart(message.upstreamProvider),
    senderGroupPart(message.providerProfile),
    senderGroupPart(message.runId),
  ].join("\u0000");
}

function assistantSenderGroupKeyForTimelineItem(
  item: TimelineItem,
  fallbackProviderHarness: string | null | undefined,
  fallbackProviderSessionId: string | null | undefined,
): string | null {
  if (item.kind === "message") {
    return assistantSenderGroupKeyForMessage(item.data);
  }
  if (item.kind === "streaming" || item.kind === "streaming_row") {
    return [
      "assistant",
      "",
      senderGroupPart(fallbackProviderHarness),
      senderGroupPart(fallbackProviderSessionId),
      "",
      "",
    ].join("\u0000");
  }
  return null;
}

const DEFAULT_ASSISTANT_GROUP_STATE = {
  reserveAssistantGutter: false,
  showSenderHeader: true,
};
type AssistantSenderGroupState = typeof DEFAULT_ASSISTANT_GROUP_STATE;

function ToolCallGroupToggleRow({
  msg,
  marker,
  senderGroupState,
  isLastInList,
  isExpanded,
  onToggle,
  contentWidthClassName,
  rowRef,
  agentRun,
  personaRuns,
  agentPersonasEnabled,
}: {
  msg: ChatMessageData;
  marker: ToolCallGroupMarker;
  senderGroupState: typeof DEFAULT_ASSISTANT_GROUP_STATE;
  isLastInList: boolean;
  isExpanded: boolean;
  onToggle: React.MouseEventHandler<HTMLButtonElement>;
  contentWidthClassName?: string | undefined;
  rowRef?: React.Ref<HTMLDivElement> | undefined;
  agentRun?: PersonaAttributedRun | null | undefined;
  personaRuns: readonly PersonaAttributedRun[];
  agentPersonasEnabled: boolean;
}) {
  return (
    <div
      ref={rowRef}
      className="px-3 w-full"
      data-chat-last-rendered-row={isLastInList ? "true" : undefined}
      style={contentContainerStyle}
    >
      <ContentShell className={contentWidthClassName}>
        <MessageItem
          role={msg.role}
          content=""
          createdAt={msg.createdAt}
          isLastInList={isLastInList}
          toolCalls={null}
          contentBlocks={null}
          providerHarness={msg.providerHarness}
          providerSessionId={msg.providerSessionId}
          upstreamProvider={msg.upstreamProvider}
          providerProfile={msg.providerProfile}
          logicalModel={msg.logicalModel}
          effectiveModelId={msg.effectiveModelId}
          logicalEffort={msg.logicalEffort}
          effectiveEffort={msg.effectiveEffort}
          inputTokens={msg.inputTokens}
          outputTokens={msg.outputTokens}
          cacheCreationTokens={msg.cacheCreationTokens}
          cacheReadTokens={msg.cacheReadTokens}
          estimatedUsd={msg.estimatedUsd}
          {...personaRunBadgeProps(
            attributedRunForId(agentRun, personaRuns, msg.runId),
            agentPersonasEnabled && senderGroupState.showSenderHeader,
            msg.runId,
          )}
          showAssistantIcon={senderGroupState.showSenderHeader}
          reserveAssistantIconSpace={senderGroupState.reserveAssistantGutter}
          showProviderMeta={senderGroupState.showSenderHeader}
          hideMeta
        >
          <ToolActivityGroupToggle
            groupKey={marker.key}
            summary={marker.summary}
            isExpanded={isExpanded}
            onToggle={onToggle}
          />
        </MessageItem>
      </ContentShell>
    </div>
  );
}

function LiveTranscriptRowItem({
  row,
  senderGroupState,
  isLastVisibleTimelineItem,
  streamingMessageCreatedAt,
  streamingTasks,
  providerHarness,
  providerSessionId,
  expandedToolGroupKeys,
  contentWidthClassName,
  rowRef,
  onToggleToolCallGroup,
  renderStreamingToolCallBlock,
}: {
  row: LiveTranscriptRow;
  senderGroupState: AssistantSenderGroupState;
  isLastVisibleTimelineItem: boolean;
  streamingMessageCreatedAt: string;
  streamingTasks: Map<string, StreamingTask> | undefined;
  providerHarness: string | null | undefined;
  providerSessionId: string | null | undefined;
  expandedToolGroupKeys: Set<string>;
  contentWidthClassName?: string | undefined;
  rowRef?: React.Ref<HTMLDivElement> | undefined;
  onToggleToolCallGroup: (groupKey: string, anchor: HTMLElement | null) => void;
  renderStreamingToolCallBlock: (block: StreamingToolUseBlock, index: number) => React.ReactNode;
}) {
  const children = (() => {
    if (row.kind === "text") {
      return (
        <>
          <TextBubble
            text={row.text}
            isUser={false}
            isStreaming
          />
          <MessageMeta
            createdAt={streamingMessageCreatedAt}
            copyableText={row.text.trim()}
          />
        </>
      );
    }
    if (row.kind === "task") {
      const task = streamingTasks?.get(row.toolUseId);
      return task ? <TaskSubagentCard task={task} /> : null;
    }
    if (row.kind === "tool_call") {
      return renderStreamingToolCallBlock(row.block, row.index);
    }
    if (row.kind === "thinking") {
      const groupKey = liveThinkingGroupKey(row.block, row.index);
      const isExpanded = expandedToolGroupKeys.has(groupKey);
      return (
        <div className="space-y-1.5 overflow-hidden">
          <ThinkingGroupToggle groupKey={groupKey} isExpanded={isExpanded}
            isSettled={row.block.isSettled ?? false} {...(row.block.durationMs != null ? { durationMs: row.block.durationMs } : {})}
            {...(row.block.estimatedTokens != null ? { estimatedTokens: row.block.estimatedTokens } : {})}
            onToggle={(event) => onToggleToolCallGroup(groupKey, event.currentTarget)} />
          {isExpanded && row.block.text ? <ThinkingWidget text={row.block.text} /> : null}
        </div>
      );
    }

    const groupKey = liveToolGroupKey(row.entries, row.taskEntries);
    const isExpanded = expandedToolGroupKeys.has(groupKey);
    const tasks = row.taskEntries
      .map((entry) => ({ entry, task: streamingTasks?.get(entry.toolUseId) }))
      .filter((item): item is { entry: typeof row.taskEntries[number]; task: StreamingTask } => (
        item.task != null
      ));
    const activityTasks: ToolActivityTask[] = tasks.map(({ task }) => ({
      toolUseId: task.toolUseId,
      toolName: task.toolName,
      ...(task.delegatedJobId ? { delegatedJobId: task.delegatedJobId } : {}),
    }));
    const summary = summarizeToolActivity({
      toolCalls: row.entries.map((entry) => entry.block.toolCall),
      tasks: activityTasks,
    });
    const activityEntries = [
      ...row.entries.map((entry) => ({ kind: "tool" as const, index: entry.index, entry })),
      ...tasks.map(({ entry, task }) => ({ kind: "task" as const, index: entry.index, task })),
    ].sort((left, right) => left.index - right.index);
    return (
      <>
        <div className="mb-2">
          <ToolActivityGroupToggle
            groupKey={groupKey}
            summary={summary}
            isExpanded={isExpanded}
            onToggle={(event) => onToggleToolCallGroup(groupKey, event.currentTarget)}
          />
        </div>
        {activityEntries.map((activity) => {
          if (activity.kind === "task") {
            return <TaskSubagentCard key={`task-${activity.task.toolUseId}`} task={activity.task} />;
          }
          return isExpanded
            ? renderStreamingToolCallBlock(activity.entry.block, activity.entry.index)
            : null;
        })}
      </>
    );
  })();

  if (!children) {
    return null;
  }

  return (
    <div
      ref={rowRef}
      className="px-3 w-full"
      data-chat-last-rendered-row={isLastVisibleTimelineItem ? "true" : undefined}
      data-chat-live-row-key={row.key}
      style={contentContainerStyle}
    >
      <ContentShell className={contentWidthClassName}>
        <MessageItem
          role="assistant"
          content=""
          createdAt={streamingMessageCreatedAt}
          isLastInList={isLastVisibleTimelineItem}
          toolCalls={null}
          contentBlocks={null}
          providerHarness={providerHarness}
          providerSessionId={providerSessionId}
          showAssistantIcon={senderGroupState.showSenderHeader}
          reserveAssistantIconSpace={senderGroupState.reserveAssistantGutter}
          showProviderMeta={senderGroupState.showSenderHeader}
          hideMeta
        >
          {children}
        </MessageItem>
      </ContentShell>
    </div>
  );
}

function isMessageAtOrAfter(candidate: ChatMessageData, marker: ChatMessageData) {
  const candidateTime = new Date(candidate.createdAt).getTime();
  const markerTime = new Date(marker.createdAt).getTime();
  return candidateTime > markerTime || (candidateTime === markerTime && candidate.id >= marker.id);
}

function latestMessageByCreatedAt(
  messages: ChatMessageData[],
  predicate: (message: ChatMessageData) => boolean,
) {
  let latest: ChatMessageData | null = null;
  let latestTime = -Infinity;

  for (const message of messages) {
    if (!predicate(message)) {
      continue;
    }
    const time = new Date(message.createdAt).getTime();
    if (
      latest === null ||
      time > latestTime ||
      (time === latestTime && message.id > latest.id)
    ) {
      latest = message;
      latestTime = time;
    }
  }

  return latest;
}

function hasRenderablePersistedContent(message: ChatMessageData) {
  if (message.content.trim().length > 0) {
    return true;
  }
  if ((message.toolCalls?.length ?? 0) > 0) {
    return true;
  }
  return (message.contentBlocks?.length ?? 0) > 0;
}

function getCurrentTurnProviderMessageId(
  messages: ChatMessageData[],
  {
    hasActiveStreaming,
    isAgentRunning,
    isFinalizing,
  }: {
    hasActiveStreaming: boolean;
    isAgentRunning: boolean;
    isFinalizing: boolean;
  },
) {
  const shouldSuppressActiveTurnSnapshot = hasActiveStreaming || isFinalizing;
  const shouldSuppressEmptyCurrentTurnSnapshot = isAgentRunning;
  if (!shouldSuppressActiveTurnSnapshot && !shouldSuppressEmptyCurrentTurnSnapshot) {
    return null;
  }

  const latestUserMessage = latestMessageByCreatedAt(
    messages,
    (message) => message.role === "user",
  );
  const latestProviderMessage = latestMessageByCreatedAt(
    messages,
    (message) => {
      if (
        !isProviderRole(message.role)
        || message.timelineSequence != null
        || isPersistedTimelineToolCallMessage(message)
      ) {
        return false;
      }
      if (!latestUserMessage) {
        return true;
      }
      return isMessageAtOrAfter(message, latestUserMessage);
    },
  );

  if (!latestProviderMessage) {
    return null;
  }

  const isEmptySnapshot = !hasRenderablePersistedContent(latestProviderMessage);
  const belongsToCurrentTurn = latestUserMessage
    ? isMessageAtOrAfter(latestProviderMessage, latestUserMessage)
    : isEmptySnapshot;

  if (!belongsToCurrentTurn) {
    return null;
  }

  if (shouldSuppressActiveTurnSnapshot) {
    return latestProviderMessage.id;
  }

  return isEmptySnapshot ? latestProviderMessage.id : null;
}

interface ChatMessageListProps {
  messages: ChatMessageData[];
  /** Conversation ID - used as key to force remount on conversation switch */
  conversationId: string | null;
  /** Absolute index of the first loaded message in the full conversation timeline */
  firstItemIndex?: number;
  /** Show failed run banner */
  failedRun?: { id: string; errorMessage: string } | null;
  /** Callback when failed run banner is dismissed */
  onDismissFailedRun?: (runId: string) => void;
  /** Is agent currently sending/responding */
  isSending: boolean;
  isAgentRunning: boolean;
  /** Short label shown beside the active typing indicator */
  typingIndicatorLabel?: string | null | undefined;
  /** Streaming tool calls to display */
  streamingToolCalls: ToolCall[];
  /** Streaming subagent tasks — Map keyed by tool_use_id */
  streamingTasks?: Map<string, StreamingTask>;
  /** Streaming content blocks (text and tool calls interleaved) */
  streamingContentBlocks?: StreamingContentBlock[];
  /** Optional timestamp to scroll to (for history mode) - scrolls to first message at or after this time */
  scrollToTimestamp?: string | null;
  /** Resolved hook events (completed + blocks) — optional, interleaved chronologically */
  hookEvents?: HookEvent[];
  /** Currently running hooks — optional, interleaved chronologically */
  activeHooks?: HookStartedEvent[];
  /** Whether the conversation is finalizing (between message_created and query refetch) */
  isFinalizing?: boolean;
  /** Context key for tool-call widget state. */
  contextKey?: string | undefined;
  /** Provider metadata for the active conversation */
  providerHarness?: string | null | undefined;
  providerSessionId?: string | null | undefined;
  agentRun?: PersonaAttributedRun | null | undefined;
  personaRuns?: readonly PersonaAttributedRun[];
  agentPersonasEnabled?: boolean;
  contentWidthClassName?: string | undefined;
  topInsetClassName?: string | undefined;
  hasOlderMessages?: boolean;
  isFetchingOlderMessages?: boolean;
  onLoadOlderMessages?: (() => void | Promise<void>) | undefined;
  initialPaintCoverKey?: string | null | undefined;
  onInitialPaintReady?: ((key: string) => void) | undefined;
}

// ============================================================================
// Component
// ============================================================================

export const ChatMessageList = forwardRef<VirtuosoHandle, ChatMessageListProps>(
  function ChatMessageList(
    {
      messages,
      conversationId,
      firstItemIndex = 0,
      failedRun,
      onDismissFailedRun,
      isSending,
      isAgentRunning,
      typingIndicatorLabel,
      streamingToolCalls,
      streamingTasks,
      streamingContentBlocks,
      scrollToTimestamp,
      hookEvents = EMPTY_HOOK_EVENTS,
      activeHooks = EMPTY_ACTIVE_HOOKS,
      isFinalizing = false,
      contextKey,
      providerHarness,
      providerSessionId,
      agentRun,
      personaRuns = [],
      agentPersonasEnabled = false,
      contentWidthClassName,
      topInsetClassName,
      hasOlderMessages = false,
      isFetchingOlderMessages = false,
      onLoadOlderMessages,
      initialPaintCoverKey = null,
      onInitialPaintReady,
    },
    ref
  ) {
    const preferredScrollBehavior = shouldUseWebkitSafeScrollBehavior()
      ? "auto"
      : "smooth";
    const lastMessage = messages[messages.length - 1] ?? null;
    const lastUserMessageId = lastMessage?.role === "user" ? lastMessage.id : null;

    // Internal ref for scroll operations
    const virtuosoRef = useRef<VirtuosoHandle>(null);
    const previousLastItemIndexRef = useRef<number | null>(null);
    // Track previous shouldFilterLastAssistant to detect false→true→false transition
    const prevShouldFilterRef = useRef(false);
    const lastUserMessageIdRef = useRef<string | null>(lastUserMessageId);
    const agentRunningRef = useRef(isAgentRunning);
    const conversationLastUserMessageIdRef = useRef<string | null>(lastUserMessageId);
    const conversationAgentRunningRef = useRef(isAgentRunning);
    const scrollerElRef = useRef<HTMLElement | null>(null);
    const scrollerResizeObserverRef = useRef<ResizeObserver | null>(null);
    const lastRenderedRowResizeObserverRef = useRef<ResizeObserver | null>(null);
    const lastRenderedRowHeightRef = useRef<number | null>(null);
    const previousTotalListHeightRef = useRef<number>(-1);
    const previousFirstItemIndexRef = useRef({ conversationId, index: firstItemIndex });
    const timestampJumpKeyRef = useRef<string | null>(null);
    const lastItemIndexRef = useRef(firstItemIndex);
    const isTestEnv = import.meta.env.VITEST;
    const [isVisuallyAtBottom, setIsVisuallyAtBottomState] = useState(true);
    const isVisuallyAtBottomRef = useRef(true);
    const [hasScrollerElement, setHasScrollerElement] = useState(false);
    const [hasScrollableOverflow, setHasScrollableOverflow] = useState(false);
    const [expandedToolGroupKeys, setExpandedToolGroupKeys] = useState<Set<string>>(
      () => new Set(),
    );
    const expandedToolGroupConversationRef = useRef<string | undefined>(conversationId);
    const thinkingIntentRef = useRef<Map<string, ThinkingGroupIntent>>(new Map());
    const transcriptRootRef = useRef<HTMLDivElement | null>(null);
    const initialPaintReadyFrameRef = useRef<number | null>(null);
    const initialPaintReadyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    useEffect(() => {
      if (expandedToolGroupConversationRef.current === conversationId) {
        return;
      }
      expandedToolGroupConversationRef.current = conversationId;
      thinkingIntentRef.current.clear();
      setExpandedToolGroupKeys(new Set());
    }, [conversationId]);

    const initialPaintReadyFallbackTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const initialPaintReadyAttemptRef = useRef(0);
    const initialPendingPaintCoverKey =
      initialPaintCoverKey && messages.length > 0 ? initialPaintCoverKey : null;
    const pendingInitialPaintCoverKeyRef = useRef<string | null>(initialPendingPaintCoverKey);
    const completedInitialPaintCoverKeyRef = useRef<string | null>(null);
    const [pendingInitialPaintCoverKey, setPendingInitialPaintCoverKeyState] =
      useState<string | null>(() => initialPendingPaintCoverKey);
    const shouldShowInitialPaintCover =
      pendingInitialPaintCoverKey !== null && messages.length > 0;

    const setPendingInitialPaintCoverKey = useCallback((nextKey: string | null) => {
      pendingInitialPaintCoverKeyRef.current = nextKey;
      setPendingInitialPaintCoverKeyState(nextKey);
    }, []);

    const setIsVisuallyAtBottom = useCallback((nextValue: boolean) => {
      if (isVisuallyAtBottomRef.current === nextValue) {
        return;
      }
      isVisuallyAtBottomRef.current = nextValue;
      setIsVisuallyAtBottomState(nextValue);
    }, []);

    const getScrollElement = useCallback(() => scrollerElRef.current, []);
    const scrollToTimelineIndex = useCallback(
      (options: { index: number; align: "start" | "end"; behavior: "auto" | "smooth" }) => {
        virtuosoRef.current?.scrollToIndex(options);
      },
      [],
    );
    const getLastTimelineIndex = useCallback(() => lastItemIndexRef.current, []);
    const [scrollControllerDeps] = useState<ChatScrollControllerDeps>(() => ({
      getScrollElement,
      scrollToIndex: scrollToTimelineIndex,
      getLastIndex: getLastTimelineIndex,
      requestFrame: (callback) => requestAnimationFrame(callback),
      cancelFrame: (frame) => cancelAnimationFrame(frame),
      onVisualBottomChange: setIsVisuallyAtBottom,
      debugLog: (event, detail) => logger.debug(`[ChatScroll] ${event}`, detail),
    }));
    const [scrollController] = useState<ChatScrollController>(() =>
      createChatScrollController({
        ...scrollControllerDeps,
      }),
    );

    const disconnectLastRenderedRowResizeObserver = useCallback(() => {
      lastRenderedRowResizeObserverRef.current?.disconnect();
      lastRenderedRowResizeObserverRef.current = null;
      lastRenderedRowHeightRef.current = null;
    }, []);

    const handleLastRenderedRowRef = useCallback((element: HTMLDivElement | null) => {
      disconnectLastRenderedRowResizeObserver();
      if (!element || typeof ResizeObserver === "undefined") {
        return;
      }

      const observer = new ResizeObserver((entries) => {
        const entry = entries.find(({ target }) => target === element);
        if (!entry) {
          return;
        }

        const previousHeight = lastRenderedRowHeightRef.current;
        const nextHeight = entry.contentRect.height;
        lastRenderedRowHeightRef.current = nextHeight;
        if (
          previousHeight !== null
          && nextHeight > previousHeight + VISUAL_BOTTOM_EPSILON_PX
        ) {
          scrollController.notifyContentGrowth();
        }
      });
      observer.observe(element);
      lastRenderedRowResizeObserverRef.current = observer;
    }, [disconnectLastRenderedRowResizeObserver, scrollController]);

    const toggleToolCallGroup = useCallback((groupKey: string, toggleElement?: HTMLElement | null) => {
      scrollController?.captureAnchor(toggleElement ?? undefined);
      setExpandedToolGroupKeys((current) => {
        const next = new Set(current);
        if (next.has(groupKey)) {
          next.delete(groupKey);
          if (isLiveThinkingGroupKey(groupKey)) {
            thinkingIntentRef.current.set(groupKey, "collapsed");
          }
        } else {
          next.add(groupKey);
          if (isLiveThinkingGroupKey(groupKey)) {
            thinkingIntentRef.current.set(groupKey, "expanded");
          }
        }
        return next;
      });
    }, [scrollController]);

    const cancelInitialPaintReadyJob = useCallback(() => {
      if (initialPaintReadyFrameRef.current !== null) {
        cancelAnimationFrame(initialPaintReadyFrameRef.current);
        initialPaintReadyFrameRef.current = null;
      }
      if (initialPaintReadyTimerRef.current !== null) {
        clearTimeout(initialPaintReadyTimerRef.current);
        initialPaintReadyTimerRef.current = null;
      }
      if (initialPaintReadyFallbackTimerRef.current !== null) {
        clearTimeout(initialPaintReadyFallbackTimerRef.current);
        initialPaintReadyFallbackTimerRef.current = null;
      }
      initialPaintReadyAttemptRef.current = 0;
    }, []);

    useEffect(
      () => () => cancelInitialPaintReadyJob(),
      [cancelInitialPaintReadyJob],
    );

    useEffect(() => {
      const nextKey = initialPaintCoverKey && messages.length > 0 ? initialPaintCoverKey : null;

      if (!nextKey) {
        completedInitialPaintCoverKeyRef.current = null;
        if (pendingInitialPaintCoverKeyRef.current !== null) {
          cancelInitialPaintReadyJob();
          setPendingInitialPaintCoverKey(null);
        }
        return;
      }

      if (completedInitialPaintCoverKeyRef.current === nextKey) {
        if (pendingInitialPaintCoverKeyRef.current !== null) {
          cancelInitialPaintReadyJob();
          setPendingInitialPaintCoverKey(null);
        }
        return;
      }

      if (pendingInitialPaintCoverKeyRef.current === nextKey) {
        return;
      }

      cancelInitialPaintReadyJob();
      setPendingInitialPaintCoverKey(nextKey);
    }, [cancelInitialPaintReadyJob, initialPaintCoverKey, messages.length, setPendingInitialPaintCoverKey]);

    const isTranscriptDomReady = useCallback(() => {
      return isTranscriptRootReadyForReveal(transcriptRootRef.current);
    }, []);

    const scheduleInitialPaintReadyCheck = useCallback(() => {
      if (!pendingInitialPaintCoverKey) {
        return;
      }
      if (initialPaintReadyFrameRef.current !== null || initialPaintReadyTimerRef.current !== null) {
        return;
      }

      const complete = () => {
        const readyKey = pendingInitialPaintCoverKeyRef.current;
        if (!readyKey) {
          return;
        }
        if (initialPaintReadyFrameRef.current !== null) {
          cancelAnimationFrame(initialPaintReadyFrameRef.current);
          initialPaintReadyFrameRef.current = null;
        }
        if (initialPaintReadyFallbackTimerRef.current !== null) {
          clearTimeout(initialPaintReadyFallbackTimerRef.current);
          initialPaintReadyFallbackTimerRef.current = null;
        }
        initialPaintReadyTimerRef.current = null;
        initialPaintReadyAttemptRef.current = 0;
        completedInitialPaintCoverKeyRef.current = readyKey;
        setPendingInitialPaintCoverKey(null);
        onInitialPaintReady?.(readyKey);
      };

      const check = () => {
        initialPaintReadyFrameRef.current = null;
        initialPaintReadyAttemptRef.current += 1;

        if (
          !isTranscriptDomReady() &&
          initialPaintReadyAttemptRef.current < INITIAL_TRANSCRIPT_PAINT_MAX_FRAMES
        ) {
          initialPaintReadyFrameRef.current = requestAnimationFrame(check);
          return;
        }

        initialPaintReadyTimerRef.current = setTimeout(complete, 0);
      };

      initialPaintReadyFrameRef.current = requestAnimationFrame(check);
      initialPaintReadyFallbackTimerRef.current = setTimeout(
        complete,
        INITIAL_TRANSCRIPT_PAINT_MAX_MS,
      );
    }, [isTranscriptDomReady, onInitialPaintReady, pendingInitialPaintCoverKey, setPendingInitialPaintCoverKey]);

    useEffect(() => {
      conversationLastUserMessageIdRef.current = lastUserMessageId;
      conversationAgentRunningRef.current = isAgentRunning;
    }, [isAgentRunning, lastUserMessageId]);

    // Forward the ref to parent
    useImperativeHandle(ref, () => virtuosoRef.current!, []);

    const { data: attachmentsMap } = useMessageAttachments(messages, conversationId, {
      enabled: !shouldShowInitialPaintCover,
    });
    const normalizedStreamingContentBlocks = useMemo(
      () => streamingContentBlocks ?? [],
      [streamingContentBlocks],
    );
    const activeRunIdSelector = useMemo(
      () => selectActiveAgentRunId(contextKey ?? ""),
      [contextKey],
    );
    const activeAgentRunId = useChatStore(activeRunIdSelector);
    const runAttributionTimingByMessageId = useMemo(
      () => buildRunAttributionTimingByMessageId(messages, activeAgentRunId),
      [messages, activeAgentRunId],
    );
    const runAttributionRunIds = useMemo(
      () => [...new Set([...runAttributionTimingByMessageId.values()].map(({ runId }) => runId))],
      [runAttributionTimingByMessageId],
    );
    const {
      data: runAttributions,
      isError: isRunAttributionsError,
      isPending: isRunAttributionsPending,
      refetch: refetchRunAttributions,
    } = useRunAttributions(runAttributionRunIds, {
      enabled: !shouldShowInitialPaintCover,
    });
    const resolvedRunAttributionTimingByMessageId = useMemo(() => {
      const resolved = new Map<string, ResolvedRunAttributionTiming>();
      for (const [messageId, timing] of runAttributionTimingByMessageId) {
        const attribution = runAttributions?.get(timing.runId) ?? null;
        resolved.set(messageId, {
          ...timing,
          startedAt: attribution?.startedAt ?? timing.startedAt,
          completedAt: attribution?.completedAt ?? timing.completedAt,
          launchRole: attribution?.launchRole ?? timing.launchRole,
          attribution,
        });
      }
      return resolved;
    }, [runAttributionTimingByMessageId, runAttributions]);
    const retryRunAttributions = useCallback(() => {
      void refetchRunAttributions();
    }, [refetchRunAttributions]);
    const delegationProjection = useMemo(
      () => projectDelegationTimelineMessages(messages, streamingTasks),
      [messages, streamingTasks],
    );
    const persistedDelegateJobIds = useMemo(
      () => persistedDelegationJobIds(delegationProjection.messages),
      [delegationProjection.messages],
    );
    const shouldHidePersistedDelegationToolCall = useCallback(
      (toolCall: ToolCall) => {
        const jobId = delegationJobIdForToolCall(toolCall);
        return shouldHideCompletedProjectOrchestrationToolCall(toolCall)
          || (jobId != null && persistedDelegateJobIds.has(jobId))
          || delegationProjection.liveAliases.has(toolCall.id);
      },
      [delegationProjection.liveAliases, persistedDelegateJobIds],
    );
    const shouldHidePersistedDelegationTask = useCallback(
      (task: StreamingTask) =>
        (task.delegatedJobId != null && persistedDelegateJobIds.has(task.delegatedJobId))
        || delegationProjection.liveAliases.has(task.toolUseId),
      [delegationProjection.liveAliases, persistedDelegateJobIds],
    );
    const liveTranscriptRows = useMemo(
      () => buildLiveTranscriptRows(
        normalizedStreamingContentBlocks,
        streamingTasks,
        shouldHidePersistedDelegationToolCall,
        shouldHidePersistedDelegationTask,
      ),
      [
        normalizedStreamingContentBlocks,
        shouldHidePersistedDelegationTask,
        shouldHidePersistedDelegationToolCall,
        streamingTasks,
      ],
    );

    useEffect(() => {
      setExpandedToolGroupKeys((current) =>
        synchronizeThinkingGroupExpansion(current, liveTranscriptRows, thinkingIntentRef.current),
      );
    }, [liveTranscriptRows]);

    const hasRenderableStreamingBlocks = useMemo(
      () => liveTranscriptRows.length > 0,
      [liveTranscriptRows],
    );
    const shouldShowActiveTypingIndicator = isSending || isAgentRunning;
    const activeTypingIndicatorLabel =
      typingIndicatorLabel?.trim()
        || (isSending ? "Starting agent" : isAgentRunning ? "Agent working" : undefined);
    const hasLiveStreamingBlocks =
      normalizedStreamingContentBlocks.length > 0 || Boolean(streamingTasks && streamingTasks.size > 0);
    const shouldShowFooterFallback =
      (isSending || isAgentRunning) && !hasRenderableStreamingBlocks && !hasLiveStreamingBlocks;
    const hasVisiblePendingToolFallback =
      shouldShowFooterFallback &&
      streamingToolCalls.some((tc) => !shouldHideCompletedProjectOrchestrationToolCall(tc));
    const hasFooterStreamingContent = hasVisiblePendingToolFallback || shouldShowActiveTypingIndicator;
    const shouldRenderStreamingContentGroup =
      hasVisiblePendingToolFallback;
    const hasLiveTranscriptContent = hasRenderableStreamingBlocks || hasFooterStreamingContent;
    const streamingMessageCreatedAt = useMemo(
      () => hasLiveTranscriptContent ? new Date().toISOString() : "",
      [hasLiveTranscriptContent],
    );

    // Build timeline data for Virtuoso. Always wraps messages as TimelineItem
    // for consistent typing. When hook events exist, they're interleaved and sorted.
    const hasHookEvents = hookEvents.length > 0 || activeHooks.length > 0;

    // Filter logic: during active streaming OR when conversation is finalizing (between
    // message_created clearing state and query refetch completing), exclude only the
    // provider snapshot for the current turn to prevent duplication with live content.
    //
    // isFinalizing is set to true (in the same React batch as clearing streaming state)
    // by useChatEvents on agent:message_created, and reset to false after 500ms. This
    // keeps the filter active through the timing window where streaming state is cleared
    // but the query refetch hasn't completed yet.
    //
    // Additionally, when isAgentRunning but no streaming content exists yet (the window
    // between DB empty-message creation and the first streaming event), filter the last
    // assistant message if its content is empty/whitespace — prevents the empty "pill" flash.
    const hasActiveStreaming = normalizedStreamingContentBlocks.length > 0 ||
                              Boolean(streamingTasks && streamingTasks.size > 0);
    const suppressedProviderMessageId = useMemo(
      () => getCurrentTurnProviderMessageId(messages, {
        hasActiveStreaming,
        isAgentRunning,
        isFinalizing,
      }),
      [hasActiveStreaming, isAgentRunning, isFinalizing, messages],
    );
    const shouldFilterCurrentProviderMessage = suppressedProviderMessageId !== null;

    const timeline = useMemo((): TimelineItem[] => {
      const items: TimelineItem[] = [];

      const filteredMessages = suppressedProviderMessageId
        ? messages.filter((msg) => msg.id !== suppressedProviderMessageId)
        : messages;
      const filteredMessageIds = new Set(filteredMessages.map((message) => message.id));
      const teamFilteredMessages = delegationProjection.messages.filter((message) =>
        filteredMessageIds.has(message.id),
      );

      const pushMessageItem = (
        msg: ChatMessageData,
        toolCallGroup?: ToolCallGroupMarker,
      ) => {
        // Enrich message with attachments if available
        const attachments = attachmentsMap?.get(msg.id);
        const enrichedMsg = attachments
          ? { ...msg, attachments }
          : msg;

        items.push({
          kind: "message",
          data: enrichedMsg,
          sortTime: new Date(msg.createdAt).getTime(),
          ...(toolCallGroup ? { toolCallGroup } : {}),
        });
      };

      for (let index = 0; index < teamFilteredMessages.length; index += 1) {
        const toolCallGroup = collectToolCallGroupRun(teamFilteredMessages, index);
        if (toolCallGroup) {
          const key = toolCallGroupKey(toolCallGroup);
          const groupedToolCalls = toolCallGroup.map(persistedTimelineToolCall);
          const summary = summarizeToolActivity({
            toolCalls: groupedToolCalls.filter(
              (toolCall): toolCall is ToolCall => toolCall != null,
            ),
          });
          toolCallGroup.forEach((msg, groupIndex) => {
            const toolCall = groupedToolCalls[groupIndex];
            pushMessageItem(msg, {
              key,
              count: toolCallGroup.length,
              summary,
              promoted: toolCall != null && isTaskToolCall(toolCall.name),
              position: groupIndex === 0 ? "toggle" : "covered",
            });
          });
          index += toolCallGroup.length - 1;
          continue;
        }

        const msg = teamFilteredMessages[index];
        if (msg) {
          pushMessageItem(msg);
        }
      }

      if (hasHookEvents) {
        for (const ev of hookEvents) {
          items.push({ kind: "hook", data: ev, sortTime: ev.timestamp });
        }
        for (const ev of activeHooks) {
          items.push({ kind: "hook", data: ev, sortTime: ev.timestamp });
        }
      }

      liveTranscriptRows.forEach((row, rowIndex) => {
        items.push({
          kind: "streaming_row",
          data: row,
          sortTime: liveTranscriptRowSortTime(
            row,
            rowIndex,
            liveTranscriptRows.length,
          ),
        });
      });

      if (hasFooterStreamingContent) {
        items.push({
          kind: "streaming",
          sortTime: Number.MAX_SAFE_INTEGER,
        });
      }

      // Sort if we interleaved any non-message items
      if (hasHookEvents || liveTranscriptRows.length > 0 || hasFooterStreamingContent) {
        items.sort((a, b) => a.sortTime - b.sortTime);
      }

      return items;
    }, [messages, suppressedProviderMessageId, hookEvents, activeHooks, hasHookEvents, attachmentsMap, delegationProjection.messages, liveTranscriptRows, hasFooterStreamingContent]);

    const timelineSenderGroups = useMemo(() => {
      let previousGroupKey: string | null = null;
      return timeline.map((item) => {
        const groupKey = assistantSenderGroupKeyForTimelineItem(
          item,
          providerHarness,
          providerSessionId,
        );
        const isContinuation = groupKey !== null && groupKey === previousGroupKey;
        previousGroupKey = groupKey;
        return {
          reserveAssistantGutter: groupKey !== null,
          showSenderHeader: !isContinuation,
        };
      });
    }, [providerHarness, providerSessionId, timeline]);

    const lastVisibleTimelineIndex = useMemo(() => {
      for (let index = timeline.length - 1; index >= 0; index -= 1) {
        const item = timeline[index];
        if (item && isVisibleTimelineItem(item, expandedToolGroupKeys)) {
          return index;
        }
      }
      return -1;
    }, [expandedToolGroupKeys, timeline]);

    const lastStreamingTimelineIndex = (() => {
      for (let index = timeline.length - 1; index >= 0; index -= 1) {
        const item = timeline[index];
        if (item?.kind === "streaming" || item?.kind === "streaming_row") {
          return index;
        }
      }
      return -1;
    })();
    const streamingSenderGroupState =
      lastStreamingTimelineIndex >= 0
        ? timelineSenderGroups[lastStreamingTimelineIndex] ?? DEFAULT_ASSISTANT_GROUP_STATE
        : DEFAULT_ASSISTANT_GROUP_STATE;

    const lastItemIndex = firstItemIndex + timeline.length - 1;
    useLayoutEffect(() => {
      lastItemIndexRef.current = lastItemIndex;
    }, [lastItemIndex]);

    useLayoutEffect(() => {
      scrollController?.restoreAnchor();
    }, [expandedToolGroupKeys, scrollController]);

    const updateScrollableOverflow = useCallback((element: HTMLElement) => {
      setHasScrollableOverflow(
        element.scrollHeight > element.clientHeight + VISUAL_BOTTOM_EPSILON_PX,
      );
    }, [setHasScrollableOverflow]);

    const handleTotalListHeightChanged = useCallback((height: number) => {
      const previousHeight = previousTotalListHeightRef.current;
      previousTotalListHeightRef.current = height;
      const scroller = scrollerElRef.current;
      if (scroller) {
        updateScrollableOverflow(scroller);
      }
      if (height > 0 && (previousHeight < 0 || height > previousHeight)) {
        scrollController?.notifyContentGrowth();
      } else {
        scrollController?.isVisuallyAtBottom();
      }
    }, [scrollController, updateScrollableOverflow]);

    const handleScrollerWheel = useCallback((event: WheelEvent) => {
      const scroller = scrollerElRef.current;
      scrollController?.notifyWheel(
        event.deltaY,
        Boolean(scroller && isNestedScrollableWheelTarget(event.target, scroller)),
      );
    }, [scrollController]);

    const handleScrollerKeyDown = useCallback((event: KeyboardEvent) => {
      if (isEditableOrInteractiveTarget(event.target)) return;
      if (["PageUp", "Home", "ArrowUp"].includes(event.key)) {
        scrollController?.notifyKeyScroll("up");
      } else if (["PageDown", "End", "ArrowDown"].includes(event.key)) {
        scrollController?.notifyKeyScroll("down");
      }
    }, [scrollController]);

    const handleScrollerPointerDown = useCallback(() => {
      scrollController?.notifyPointerDown();
    }, [scrollController]);

    const handleScrollerPointerUp = useCallback(() => {
      scrollController?.notifyPointerUp();
    }, [scrollController]);

    const handleScrollReconcile = useCallback(() => {
      const scroller = scrollerElRef.current;
      if (scroller) {
        updateScrollableOverflow(scroller);
      }
      scrollController?.notifyScroll();
    }, [scrollController, updateScrollableOverflow]);

    const disconnectScrollerResizeObserver = useCallback(() => {
      scrollerResizeObserverRef.current?.disconnect();
      scrollerResizeObserverRef.current = null;
    }, []);

    const handleScrollerRef = useCallback((el: Window | HTMLElement | null) => {
      const previous = scrollerElRef.current;
      if (previous && previous !== el) {
        previous.removeEventListener("scroll", handleScrollReconcile);
        previous.removeEventListener("wheel", handleScrollerWheel);
        previous.removeEventListener("pointerdown", handleScrollerPointerDown);
        previous.removeEventListener("pointerup", handleScrollerPointerUp);
        previous.removeEventListener("pointercancel", handleScrollerPointerUp);
        previous.removeEventListener("keydown", handleScrollerKeyDown);
        disconnectScrollerResizeObserver();
        scrollController?.detach();
      }
      if (!(el instanceof HTMLElement)) {
        scrollerElRef.current = null;
        setHasScrollerElement(false);
        setHasScrollableOverflow(false);
        return;
      }
      if (previous === el) {
        scrollController?.attach(el);
        scrollController?.notifyScroll();
        return;
      }
      scrollerElRef.current = el;
      setHasScrollerElement(true);
      updateScrollableOverflow(el);
      el.addEventListener("scroll", handleScrollReconcile, { passive: true });
      el.addEventListener("wheel", handleScrollerWheel, { passive: true });
      el.addEventListener("pointerdown", handleScrollerPointerDown, { passive: true });
      el.addEventListener("pointerup", handleScrollerPointerUp, { passive: true });
      el.addEventListener("pointercancel", handleScrollerPointerUp, { passive: true });
      el.addEventListener("keydown", handleScrollerKeyDown);
      scrollController?.attach(el);
      if (typeof ResizeObserver !== "undefined") {
        const observer = new ResizeObserver(() => {
          updateScrollableOverflow(el);
          scrollController?.notifyContainerResize();
        });
        observer.observe(el);
        scrollerResizeObserverRef.current = observer;
      }
    }, [
      disconnectScrollerResizeObserver,
      handleScrollerKeyDown,
      handleScrollerPointerDown,
      handleScrollerPointerUp,
      handleScrollerWheel,
      handleScrollReconcile,
      scrollController,
      setHasScrollerElement,
      setHasScrollableOverflow,
      updateScrollableOverflow,
    ]);

    useEffect(() => () => {
      scrollController.detach();
      disconnectScrollerResizeObserver();
      disconnectLastRenderedRowResizeObserver();
    }, [
      disconnectLastRenderedRowResizeObserver,
      disconnectScrollerResizeObserver,
      scrollController,
    ]);

    useEffect(() => {
      previousTotalListHeightRef.current = -1;
      previousLastItemIndexRef.current = null;
      timestampJumpKeyRef.current = null;
      setHasScrollableOverflow(false);
      scrollController?.reset();
    }, [conversationId, scrollController]);

    useEffect(() => {
      const previous = previousFirstItemIndexRef.current;
      previousFirstItemIndexRef.current = { conversationId, index: firstItemIndex };
      if (conversationId === previous.conversationId && firstItemIndex < previous.index) {
        scrollController.notifyPrepend();
      }
    }, [conversationId, firstItemIndex, scrollController]);

    useEffect(() => {
      if (!lastUserMessageId) {
        lastUserMessageIdRef.current = null;
        return;
      }
      if (lastUserMessageIdRef.current === lastUserMessageId) return;
      lastUserMessageIdRef.current = lastUserMessageId;
      scrollController.pinForUserIntent("new-user-message", "auto");
    }, [lastUserMessageId, scrollController]);

    useEffect(() => {
      if (isAgentRunning && !agentRunningRef.current) {
        scrollController?.requestPin("streaming-started", preferredScrollBehavior);
      }
      agentRunningRef.current = isAgentRunning;
    }, [isAgentRunning, preferredScrollBehavior, scrollController]);

    // Scroll to specific timestamp for history mode (time-travel feature)
    // Finds the first message at or after the given timestamp and scrolls to it
    useEffect(() => {
      if (!scrollToTimestamp) {
        timestampJumpKeyRef.current = null;
        return;
      }
      if (messages.length === 0) return;

      const timestampJumpKey = `${conversationId}:${scrollToTimestamp}`;
      if (timestampJumpKeyRef.current === timestampJumpKey) return;

      const targetTime = new Date(scrollToTimestamp).getTime();
      const targetIndex = messages.findIndex(
        (msg) => new Date(msg.createdAt).getTime() >= targetTime
      );

      if (targetIndex >= 0) {
        timestampJumpKeyRef.current = timestampJumpKey;
        scrollController?.jumpToIndex(firstItemIndex + targetIndex);
      }
      return undefined;
    }, [conversationId, scrollController, scrollToTimestamp, messages, firstItemIndex]);

    // When filter clears (streaming/finalizing ends), keep sticky users pinned
    // through the finalized assistant reveal without pulling manual-away users.
    useEffect(() => {
      if (scrollToTimestamp) return;
      if (prevShouldFilterRef.current && !shouldFilterCurrentProviderMessage) {
        scrollController?.requestPin("finalized-provider-message-revealed", "auto");
      }
      prevShouldFilterRef.current = shouldFilterCurrentProviderMessage;
    }, [scrollController, shouldFilterCurrentProviderMessage, scrollToTimestamp]);

    const startReachedHandler =
      hasOlderMessages && onLoadOlderMessages
        ? (_index: number) => {
            scrollController?.notifyPrepend();
            void onLoadOlderMessages();
          }
        : null;
    const virtuosoStartReachedProps = startReachedHandler === null
      ? {}
      : { startReached: startReachedHandler };
    const shouldShowScrollToBottom =
      hasScrollerElement && hasScrollableOverflow && !isVisuallyAtBottom;
    const handleScrollToBottomClick = useCallback(() => {
      scrollController?.scrollToBottomClicked();
    }, [scrollController]);
    const handleScrollToBottomWheel = useCallback(
      (event: React.WheelEvent<HTMLButtonElement>) => {
        if (!shouldShowScrollToBottom) {
          return;
        }
        event.preventDefault();
        scrollController?.notifyWheel(event.deltaY, false);
        scrollController?.forwardWheel(event.deltaX, event.deltaY);
        scrollController?.notifyScroll();
      },
      [scrollController, shouldShowScrollToBottom],
    );

    const handleRangeChanged = useCallback(
      (range: ListRange) => {
        if (timeline.length > 0 && range.endIndex >= range.startIndex) {
          scheduleInitialPaintReadyCheck();
        }
      },
      [scheduleInitialPaintReadyCheck, timeline.length],
    );

    useEffect(() => {
      if (!shouldShowInitialPaintCover) {
        return;
      }
      scheduleInitialPaintReadyCheck();
    }, [scheduleInitialPaintReadyCheck, shouldShowInitialPaintCover]);

    useEffect(() => {
      const previousLastItemIndex = previousLastItemIndexRef.current;
      previousLastItemIndexRef.current = lastItemIndex;

      if (
        scrollToTimestamp ||
        timeline.length === 0 ||
        previousLastItemIndex === null ||
        lastItemIndex <= previousLastItemIndex
      ) {
        return;
      }

      scrollController?.requestPin("new-timeline-item-appended", preferredScrollBehavior);
    }, [lastItemIndex, preferredScrollBehavior, scrollController, scrollToTimestamp, timeline.length]);

    const renderStreamingToolCallBlock = useCallback(
      (
        block: StreamingToolUseBlock,
        idx: number,
      ) => {
        if (isDiffToolCall(block.toolCall.name) && block.toolCall.arguments != null) {
          return (
            <DiffToolCallView
              key={`streaming-tool-${idx}`}
              toolCall={block.toolCall}
              isStreaming={block.toolCall.result == null && !block.toolCall.error}
              className="mb-2"
            />
          );
        }
        return (
          <ToolCallIndicator
            key={`streaming-tool-${idx}`}
            toolCall={block.toolCall}
            isStreaming={block.toolCall.result == null && !block.toolCall.error}
            className="mb-2"
          />
        );
      },
      [],
    );

    const footerContent = useMemo(() => {
      if (!hasFooterStreamingContent) {
        return null;
      }
      if (!shouldRenderStreamingContentGroup && !shouldShowActiveTypingIndicator) {
        return null;
      }
      const visibleFallbackToolCalls = shouldShowFooterFallback
        ? streamingToolCalls
          .map((toolCall, index) => ({ toolCall, index }))
          .filter(({ toolCall }) => !shouldHideCompletedProjectOrchestrationToolCall(toolCall))
        : [];
      const fallbackToolGroupKey = visibleFallbackToolCalls.length > 0
        ? [
          "streaming-pending-tool-group",
          visibleFallbackToolCalls[0]?.toolCall.id || visibleFallbackToolCalls[0]?.index || "empty",
        ].join(":")
        : null;
      const isFallbackToolGroupExpanded =
        fallbackToolGroupKey != null && expandedToolGroupKeys.has(fallbackToolGroupKey);
      const fallbackSummary = summarizeToolActivity({
        toolCalls: visibleFallbackToolCalls.map(({ toolCall }) => toolCall),
      });
      return (
        <>
          {shouldRenderStreamingContentGroup && (
            <MessageItem
              role="assistant"
              content=""
              createdAt={streamingMessageCreatedAt}
              isLastInList={!shouldShowActiveTypingIndicator}
              toolCalls={null}
              contentBlocks={null}
              providerHarness={providerHarness}
              providerSessionId={providerSessionId}
              showAssistantIcon={streamingSenderGroupState.showSenderHeader}
              reserveAssistantIconSpace={streamingSenderGroupState.reserveAssistantGutter}
              showProviderMeta={streamingSenderGroupState.showSenderHeader}
              hideMeta
            >
              {/* Fallback when agent is running but no content blocks yet:
                  Tool calls pending show immediate visibility into what agent is doing. */}
              {fallbackToolGroupKey != null && (
                <>
                  <div className="mb-2">
                    <ToolActivityGroupToggle
                      groupKey={fallbackToolGroupKey}
                      summary={fallbackSummary}
                      isExpanded={isFallbackToolGroupExpanded}
                      onToggle={(event) => toggleToolCallGroup(fallbackToolGroupKey, event.currentTarget)}
                    />
                  </div>
                  {isFallbackToolGroupExpanded
                    ? visibleFallbackToolCalls.map(({ toolCall, index }) => (
                      <ToolCallIndicator
                        key={`pending-tool-${index}`}
                        toolCall={toolCall}
                        isStreaming={toolCall.result == null && !toolCall.error}
                        className="mb-2"
                      />
                    ))
                    : null}
                </>
              )}
            </MessageItem>
          )}
          {shouldShowActiveTypingIndicator && (
            <TypingIndicator label={activeTypingIndicatorLabel} storeKey={contextKey} />
          )}
        </>
      );
    }, [
      hasFooterStreamingContent,
      shouldRenderStreamingContentGroup,
      expandedToolGroupKeys,
      providerHarness,
      providerSessionId,
      streamingSenderGroupState,
      activeTypingIndicatorLabel,
      contextKey,
      shouldShowActiveTypingIndicator,
      shouldShowFooterFallback,
      streamingMessageCreatedAt,
      streamingToolCalls,
      toggleToolCallGroup,
    ]);

    // Memoize Virtuoso components to prevent infinite re-render loop.
    // Inline object literals create new references every render, causing Virtuoso
    // to re-mount Header → layout change → atBottomStateChange → re-render → loop.
    const virtuosoComponents = useMemo(() => ({
      Scroller: ChatVirtuosoScroller,
      Header: () => (
        <div
          className={cn("px-3 w-full", topInsetClassName ?? "pt-3")}
          style={contentContainerStyle}
        >
          <ContentShell className={contentWidthClassName}>
            {/* Show failed run banner if last run failed */}
            {failedRun?.errorMessage && onDismissFailedRun && (
              <FailedRunBanner
                errorMessage={failedRun.errorMessage}
                onDismiss={() => onDismissFailedRun(failedRun.id)}
              />
            )}
          </ContentShell>
        </div>
      ),
    }), [
      contentWidthClassName,
      failedRun, onDismissFailedRun,
      topInsetClassName,
    ]);

    // Memoize itemContent for virtualized rendering.
    const renderItem = useCallback((index: number, item: TimelineItem) => {
      const timelineIndex = index - firstItemIndex;
      const isLastVisibleTimelineItem = timelineIndex === lastVisibleTimelineIndex;
      if (item.kind === "hook") {
        return (
          <div className="px-3 w-full" style={contentContainerStyle}>
            <ContentShell className={contentWidthClassName}>
              <HookEventMessage event={item.data} />
            </ContentShell>
          </div>
        );
      }
      if (item.kind === "streaming_row") {
        return (
          <LiveTranscriptRowItem
            row={item.data}
            senderGroupState={timelineSenderGroups[timelineIndex] ?? DEFAULT_ASSISTANT_GROUP_STATE}
            isLastVisibleTimelineItem={isLastVisibleTimelineItem}
            streamingMessageCreatedAt={streamingMessageCreatedAt}
            streamingTasks={streamingTasks}
            providerHarness={providerHarness}
            providerSessionId={providerSessionId}
            expandedToolGroupKeys={expandedToolGroupKeys}
            contentWidthClassName={contentWidthClassName}
            rowRef={isLastVisibleTimelineItem ? handleLastRenderedRowRef : undefined}
            onToggleToolCallGroup={toggleToolCallGroup}
            renderStreamingToolCallBlock={renderStreamingToolCallBlock}
          />
        );
      }
      if (item.kind === "streaming") {
        if (!footerContent) {
          return null;
        }
        return (
          <div className="px-3 pb-3 w-full relative" style={contentContainerStyle}>
            <ContentShell className={contentWidthClassName}>
              {footerContent}
            </ContentShell>
          </div>
        );
      }
      if (isCollapsedToolCallGroupCoveredItem(item, expandedToolGroupKeys)) {
        return null;
      }
      const msg = item.data;
      const senderGroupState =
        timelineSenderGroups[timelineIndex] ?? DEFAULT_ASSISTANT_GROUP_STATE;
      const toolCallGroup = item.toolCallGroup;
      const isExpandedToolCallGroup =
        toolCallGroup != null && expandedToolGroupKeys.has(toolCallGroup.key);
      const groupToggleRow = toolCallGroup?.position === "toggle"
        ? (
          <ToolCallGroupToggleRow
            msg={msg}
            marker={toolCallGroup}
            senderGroupState={senderGroupState}
            isLastInList={isLastVisibleTimelineItem && !isExpandedToolCallGroup}
            isExpanded={isExpandedToolCallGroup}
            onToggle={(event) => toggleToolCallGroup(toolCallGroup.key, event.currentTarget)}
            contentWidthClassName={contentWidthClassName}
            agentRun={agentRun}
            personaRuns={personaRuns}
            agentPersonasEnabled={agentPersonasEnabled}
            rowRef={
              isLastVisibleTimelineItem && !isExpandedToolCallGroup
                ? handleLastRenderedRowRef
                : undefined
            }
          />
        )
        : null;

      if (groupToggleRow && !isExpandedToolCallGroup && !toolCallGroup?.promoted) {
        return groupToggleRow;
      }

      const messageMetadata = parseMessageMetadata(msg.metadata);
      const composerReferences = parseComposerReferencesFromMetadata(messageMetadata);
      const effectiveSenderGroupState =
        toolCallGroup?.position === "toggle" && isExpandedToolCallGroup
          ? { ...senderGroupState, showSenderHeader: false }
          : senderGroupState;

      const messageRow = (
        <div
          ref={isLastVisibleTimelineItem ? handleLastRenderedRowRef : undefined}
          className="px-3 w-full"
          data-chat-last-rendered-row={isLastVisibleTimelineItem ? "true" : undefined}
          style={contentContainerStyle}
        >
          <ContentShell className={contentWidthClassName}>
            <MessageItem
              role={msg.role}
              content={msg.content}
              createdAt={msg.createdAt}
              isLastInList={isLastVisibleTimelineItem}
              toolCalls={msg.toolCalls ?? null}
              contentBlocks={msg.contentBlocks ?? null}
              groupContentBlockToolCalls={toolCallGroup == null}
              {...(msg.attachments && { attachments: msg.attachments })}
              {...(composerReferences ? { composerReferences } : {})}
              providerHarness={msg.providerHarness}
              providerSessionId={msg.providerSessionId}
              upstreamProvider={msg.upstreamProvider}
              providerProfile={msg.providerProfile}
              logicalModel={msg.logicalModel}
              effectiveModelId={msg.effectiveModelId}
              logicalEffort={msg.logicalEffort}
              effectiveEffort={msg.effectiveEffort}
              inputTokens={msg.inputTokens}
              outputTokens={msg.outputTokens}
              cacheCreationTokens={msg.cacheCreationTokens}
              cacheReadTokens={msg.cacheReadTokens}
              estimatedUsd={msg.estimatedUsd}
              {...personaRunBadgeProps(
                attributedRunForId(agentRun, personaRuns, msg.runId),
                agentPersonasEnabled &&
                  effectiveSenderGroupState.showSenderHeader,
                msg.runId,
              )}
              showAssistantIcon={effectiveSenderGroupState.showSenderHeader}
              reserveAssistantIconSpace={effectiveSenderGroupState.reserveAssistantGutter}
              showProviderMeta={effectiveSenderGroupState.showSenderHeader}
            />
            {resolvedRunAttributionTimingByMessageId.has(msg.id) ? <RunAttributionWidget {...resolvedRunAttributionTimingByMessageId.get(msg.id)!} isAttributionPending={isRunAttributionsPending} isAttributionError={isRunAttributionsError} retryAttribution={retryRunAttributions} /> : null}
          </ContentShell>
        </div>
      );
      return groupToggleRow ? (
        <>
          {groupToggleRow}
          {messageRow}
        </>
      ) : messageRow;
    }, [
      contentWidthClassName,
      agentPersonasEnabled,
      agentRun,
      personaRuns,
      expandedToolGroupKeys,
      firstItemIndex,
      footerContent,
      handleLastRenderedRowRef,
      lastVisibleTimelineIndex,
      providerHarness,
      providerSessionId,
      renderStreamingToolCallBlock,
      resolvedRunAttributionTimingByMessageId,
      isRunAttributionsError,
      isRunAttributionsPending,
      retryRunAttributions,
      streamingMessageCreatedAt,
      streamingTasks,
      timelineSenderGroups,
      toggleToolCallGroup,
    ]);

    if (isTestEnv) {
      return (
        <div
          ref={transcriptRootRef}
          className="flex-1 overflow-hidden relative"
          data-testid="integrated-chat-messages"
        >
          {shouldShowInitialPaintCover && (
            <ConversationTranscriptPlaceholders
              contentWidthClassName={contentWidthClassName}
              className="pointer-events-none absolute inset-0 z-10 bg-[var(--bg-base)]"
              testId="chat-transcript-settling-placeholders"
              ariaHidden
            />
          )}
          <div
            className={cn("px-3 w-full", topInsetClassName ?? "pt-3")}
            style={contentContainerStyle}
          >
            <ContentShell className={contentWidthClassName}>
              {failedRun?.errorMessage && onDismissFailedRun && (
                <FailedRunBanner
                  errorMessage={failedRun.errorMessage}
                  onDismiss={() => onDismissFailedRun(failedRun.id)}
                />
              )}
            </ContentShell>
          </div>

          {timeline.map((item, index) => {
            if (item.kind === "hook") {
              return (
                <div key={`${item.kind}-${item.sortTime}-${index}`} className="px-3 w-full" style={contentContainerStyle}>
                  <ContentShell className={contentWidthClassName}>
                    <HookEventMessage event={item.data} />
                  </ContentShell>
                </div>
              );
            }
            if (item.kind === "streaming_row") {
              return (
                <LiveTranscriptRowItem
                  key={item.data.key}
                  row={item.data}
                  senderGroupState={timelineSenderGroups[index] ?? DEFAULT_ASSISTANT_GROUP_STATE}
                  isLastVisibleTimelineItem={index === lastVisibleTimelineIndex}
                  streamingMessageCreatedAt={streamingMessageCreatedAt}
                  streamingTasks={streamingTasks}
                  providerHarness={providerHarness}
                  providerSessionId={providerSessionId}
                  expandedToolGroupKeys={expandedToolGroupKeys}
                  contentWidthClassName={contentWidthClassName}
                  onToggleToolCallGroup={toggleToolCallGroup}
                  renderStreamingToolCallBlock={renderStreamingToolCallBlock}
                />
              );
            }
            if (item.kind === "streaming") {
              if (!footerContent) {
                return null;
              }
              return (
                <div key="streaming-live" className="px-3 pb-3 w-full relative" style={contentContainerStyle}>
                  <ContentShell className={contentWidthClassName}>
                    {footerContent}
                  </ContentShell>
                </div>
              );
            }
            if (isCollapsedToolCallGroupCoveredItem(item, expandedToolGroupKeys)) {
              return null;
            }
            const msg = item.data;
            const senderGroupState =
              timelineSenderGroups[index] ?? DEFAULT_ASSISTANT_GROUP_STATE;
            const toolCallGroup = item.toolCallGroup;
            const isExpandedToolCallGroup =
              toolCallGroup != null && expandedToolGroupKeys.has(toolCallGroup.key);
            const isLastVisibleTimelineItem = index === lastVisibleTimelineIndex;
            const groupToggleRow = toolCallGroup?.position === "toggle"
              ? (
                <ToolCallGroupToggleRow
                  key={`tool-call-group-${toolCallGroup.key}`}
                  msg={msg}
                  marker={toolCallGroup}
                  senderGroupState={senderGroupState}
                  isLastInList={isLastVisibleTimelineItem && !isExpandedToolCallGroup}
                  isExpanded={isExpandedToolCallGroup}
                  onToggle={(event) => toggleToolCallGroup(toolCallGroup.key, event.currentTarget)}
                  contentWidthClassName={contentWidthClassName}
                  agentRun={agentRun}
                  personaRuns={personaRuns}
                  agentPersonasEnabled={agentPersonasEnabled}
                />
              )
              : null;

            if (groupToggleRow && !isExpandedToolCallGroup && !toolCallGroup?.promoted) {
              return groupToggleRow;
            }

            const messageMetadata = parseMessageMetadata(msg.metadata);
            const composerReferences = parseComposerReferencesFromMetadata(messageMetadata);
            const effectiveSenderGroupState =
              toolCallGroup?.position === "toggle" && isExpandedToolCallGroup
                ? { ...senderGroupState, showSenderHeader: false }
                : senderGroupState;

            const messageRow = (
              <div
                key={`message-${msg.id}`}
                className="px-3 w-full"
                data-chat-last-rendered-row={isLastVisibleTimelineItem ? "true" : undefined}
                style={contentContainerStyle}
              >
                <ContentShell className={contentWidthClassName}>
                  <MessageItem
                    role={msg.role}
                    content={msg.content}
                    createdAt={msg.createdAt}
                    isLastInList={isLastVisibleTimelineItem}
                    toolCalls={msg.toolCalls ?? null}
                    contentBlocks={msg.contentBlocks ?? null}
                    groupContentBlockToolCalls={toolCallGroup == null}
                    {...(msg.attachments && { attachments: msg.attachments })}
                    {...(composerReferences ? { composerReferences } : {})}
                    providerHarness={msg.providerHarness}
                    providerSessionId={msg.providerSessionId}
                    upstreamProvider={msg.upstreamProvider}
                    providerProfile={msg.providerProfile}
                    logicalModel={msg.logicalModel}
                    effectiveModelId={msg.effectiveModelId}
                    logicalEffort={msg.logicalEffort}
                    effectiveEffort={msg.effectiveEffort}
                    inputTokens={msg.inputTokens}
                    outputTokens={msg.outputTokens}
                    cacheCreationTokens={msg.cacheCreationTokens}
                    cacheReadTokens={msg.cacheReadTokens}
                    estimatedUsd={msg.estimatedUsd}
                    {...personaRunBadgeProps(
                      attributedRunForId(agentRun, personaRuns, msg.runId),
                      agentPersonasEnabled &&
                        effectiveSenderGroupState.showSenderHeader,
                      msg.runId,
                    )}
                    showAssistantIcon={effectiveSenderGroupState.showSenderHeader}
                    reserveAssistantIconSpace={effectiveSenderGroupState.reserveAssistantGutter}
                    showProviderMeta={effectiveSenderGroupState.showSenderHeader}
                  />
                  {resolvedRunAttributionTimingByMessageId.has(msg.id) ? <RunAttributionWidget {...resolvedRunAttributionTimingByMessageId.get(msg.id)!} isAttributionPending={isRunAttributionsPending} isAttributionError={isRunAttributionsError} retryAttribution={retryRunAttributions} /> : null}
                </ContentShell>
              </div>
            );
            return groupToggleRow ? (
              <React.Fragment key={`expanded-tool-call-group-${toolCallGroup?.key ?? msg.id}`}>
                {groupToggleRow}
                {messageRow}
              </React.Fragment>
            ) : messageRow;
          })}

          <ScrollToBottomControl
            visible={shouldShowScrollToBottom}
            onClick={handleScrollToBottomClick}
            onWheel={handleScrollToBottomWheel}
          />
        </div>
      );
    }

    return (
      <ToolCallStoreKeyContext.Provider value={contextKey ?? null}>
      <div
        ref={transcriptRootRef}
        className="flex-1 overflow-hidden relative"
        data-testid="integrated-chat-messages"
      >
        {shouldShowInitialPaintCover && (
          <ConversationTranscriptPlaceholders
            contentWidthClassName={contentWidthClassName}
            className="pointer-events-none absolute inset-0 z-10 bg-[var(--bg-base)]"
            testId="chat-transcript-settling-placeholders"
            ariaHidden
          />
        )}
        <Virtuoso
          // Key forces complete remount when conversation changes - prevents scroll animation conflicts
          key={conversationId ?? "empty"}
          ref={virtuosoRef}
          scrollerRef={handleScrollerRef}
          data={timeline}
          firstItemIndex={firstItemIndex}
          // Start at the last item on mount
          initialTopMostItemIndex={timeline.length > 0 ? lastItemIndex : 0}
          followOutput={false}
          rangeChanged={handleRangeChanged}
          totalListHeightChanged={handleTotalListHeightChanged}
          {...virtuosoStartReachedProps}
          alignToBottom
          className="h-full"
          components={virtuosoComponents}
          itemContent={renderItem}
        />
        {isFetchingOlderMessages && (
          <div className="absolute top-2 left-0 right-0 flex justify-center pointer-events-none">
            <span
              className="rounded-full px-3 py-1 text-[0.6875rem]"
              style={{
                backgroundColor: "color-mix(in srgb, var(--bg-surface) 94%, transparent)",
                border: "1px solid var(--border-subtle)",
                color: "var(--text-secondary)",
              }}
            >
              Loading earlier messages...
            </span>
          </div>
        )}
        {/* Kept outside Virtuoso and always mounted so visibility changes do not rebuild the transcript. */}
        <ScrollToBottomControl
          visible={shouldShowScrollToBottom}
          onClick={handleScrollToBottomClick}
          onWheel={handleScrollToBottomWheel}
        />
      </div>
      </ToolCallStoreKeyContext.Provider>
    );
  }
);
