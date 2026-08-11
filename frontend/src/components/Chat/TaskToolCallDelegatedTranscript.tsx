import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { chatApi } from "@/api/chat";
import { mergeStreamingTextSnapshot } from "@/hooks/chat-active-state";
import { isProviderRole } from "@/lib/chat/provider-role";
import {
  invalidateConversationDataQueries,
  useConversationHistoryWindow,
} from "@/hooks/useChat";
import { useEventBus } from "@/providers/EventProvider";
import { TaskCardTranscriptView } from "./TaskCardTranscript";
import {
  buildTaskCardTranscriptEntriesFromConversation,
  buildTaskCardTranscriptEntryFromToolCall,
} from "./TaskCardTranscript.utils";
import type { ToolCall } from "./ToolCallIndicator";
import {
  mergeDelegationToolCalls,
  parseToolResultId,
} from "./delegation-tool-calls";

function liveToolCallFromPayload(payload: {
  tool_name?: string | undefined;
  tool_id?: string | undefined;
  arguments?: unknown;
  result?: unknown;
}): ToolCall | null {
  if (!payload.tool_name) return null;
  const resultToolId = parseToolResultId(payload.tool_name);
  return {
    id: payload.tool_id ?? resultToolId ?? `${payload.tool_name}:live`,
    name: payload.tool_name,
    arguments: payload.arguments ?? {},
    ...(payload.result != null ? { result: payload.result } : {}),
  };
}

function liveTaskToolCallFromPayload(payload: {
  tool_use_id?: string | undefined;
  description?: string | undefined;
  subagent_type?: string | undefined;
  model?: string | undefined;
  status?: string | undefined;
  text_output?: string | undefined;
  total_tokens?: number | undefined;
  total_tool_uses?: number | undefined;
  duration_ms?: number | undefined;
  delegated_job_id?: string | undefined;
}): ToolCall | null {
  if (!payload.tool_use_id) return null;
  const isTerminal = payload.status != null && payload.status !== "running";
  return {
    id: payload.tool_use_id,
    name: payload.delegated_job_id ? "delegate_start" : "Task",
    arguments: {
      description: payload.description,
      subagent_type: payload.subagent_type,
      model: payload.model,
      ...(payload.delegated_job_id ? { job_id: payload.delegated_job_id } : {}),
    },
    ...(isTerminal
      ? {
          result: {
            status: payload.status,
            text: payload.text_output,
            total_tokens: payload.total_tokens,
            total_tool_use_count: payload.total_tool_uses,
            total_duration_ms: payload.duration_ms,
            ...(payload.delegated_job_id ? { job_id: payload.delegated_job_id } : {}),
          },
        }
      : {}),
  };
}

function mergeLiveToolCalls(toolCalls: ToolCall[]): ToolCall[] {
  const merged = new Map<string, ToolCall>();
  for (const toolCall of toolCalls) {
    merged.set(toolCall.id, { ...merged.get(toolCall.id), ...toolCall });
  }
  return mergeDelegationToolCalls([...merged.values()]);
}

function FallbackText({ text }: { text: string }) {
  return (
    <pre
      className="text-[0.6875rem] px-2 py-1.5 rounded overflow-x-auto max-h-64"
      style={{
        backgroundColor: "var(--bg-surface)",
        color: "var(--text-secondary)",
        fontFamily: "var(--font-mono)",
        wordBreak: "break-word",
        whiteSpace: "pre-wrap",
      }}
    >
      {text}
    </pre>
  );
}

export function TaskToolCallDelegatedTranscript({
  conversationId,
  delegatedAgentRunId,
  fallbackText,
}: {
  conversationId: string;
  delegatedAgentRunId?: string | undefined;
  fallbackText: string | undefined;
}) {
  const bus = useEventBus();
  const queryClient = useQueryClient();
  const delegatedConversation = useConversationHistoryWindow(conversationId, {
    pageSize: 40,
  });
  const messages = delegatedConversation.data?.messages ?? [];
  const [liveText, setLiveText] = useState("");
  const liveTextRef = useRef("");
  const [finalizedLiveMessageId, setFinalizedLiveMessageId] = useState<
    string | null
  >(null);
  const [activeStateSettled, setActiveStateSettled] = useState(false);
  const [activeStateError, setActiveStateError] = useState(false);
  const [isChildActive, setIsChildActive] = useState(false);
  const [liveToolCalls, setLiveToolCalls] = useState<ToolCall[]>([]);

  useEffect(() => {
    let cancelled = false;
    liveTextRef.current = "";
    setLiveText("");
    setFinalizedLiveMessageId(null);
    setActiveStateSettled(false);
    setActiveStateError(false);
    setIsChildActive(false);
    setLiveToolCalls([]);

    void chatApi
      .getConversationActiveState(conversationId)
      .then((activeState) => {
        if (cancelled) return;
        if (delegatedAgentRunId && activeState.runId !== delegatedAgentRunId) {
          return;
        }
        setIsChildActive(activeState.is_active);
        const recoveredToolCalls = activeState.tool_calls
            .map((toolCall) => liveToolCallFromPayload(toolCall as {
              tool_name?: string;
              name?: string;
              tool_id?: string;
              id?: string;
              arguments?: unknown;
              result?: unknown;
            }))
            .map((toolCall, index) => {
              const raw = activeState.tool_calls[index] as Record<string, unknown>;
              return toolCall ?? liveToolCallFromPayload({
                tool_name: typeof raw.name === "string" ? raw.name : undefined,
                tool_id: typeof raw.id === "string" ? raw.id : undefined,
                arguments: raw.arguments,
                result: raw.result,
              });
            })
            .filter((toolCall): toolCall is ToolCall => toolCall != null);
        const recoveredTasks = activeState.streaming_tasks
          .map(liveTaskToolCallFromPayload)
          .filter((toolCall): toolCall is ToolCall => toolCall != null);
        setLiveToolCalls(
          mergeLiveToolCalls([...recoveredToolCalls, ...recoveredTasks]),
        );
        if (activeState.partial_text.trim().length > 0) {
          setLiveText((current) => {
            const next = mergeStreamingTextSnapshot(activeState.partial_text, current);
            liveTextRef.current = next;
            return next;
          });
        }
      })
      .catch(() => {
        if (!cancelled) {
          setActiveStateError(true);
        }
      })
      .finally(() => {
        if (!cancelled) {
          setActiveStateSettled(true);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [conversationId, delegatedAgentRunId]);

  useEffect(() => {
    const invalidateTranscript = (payload: {
      conversation_id?: string;
      message_id?: string;
      role?: string;
    }) => {
      if (payload.conversation_id !== conversationId) {
        return;
      }
      if (payload.role === "user") {
        liveTextRef.current = "";
        setLiveText("");
        setFinalizedLiveMessageId(null);
      } else if (
        payload.message_id &&
        isProviderRole(payload.role) &&
        liveTextRef.current.trim().length > 0
      ) {
        setFinalizedLiveMessageId(payload.message_id);
      }
      invalidateConversationDataQueries(queryClient, conversationId);
    };

    const appendLiveChunk = (payload: {
      conversation_id?: string;
      run_id?: string;
      text?: string;
    }) => {
      if (
        payload.conversation_id !== conversationId
        || !payload.text
        || (delegatedAgentRunId && payload.run_id !== delegatedAgentRunId)
      ) {
        return;
      }
      setIsChildActive(true);
      setLiveText((current) => {
        const next = mergeStreamingTextSnapshot(current, payload.text ?? "");
        liveTextRef.current = next;
        return next;
      });
    };

    const updateLiveToolCall = (payload: {
      conversation_id?: string;
      run_id?: string;
      tool_name?: string;
      tool_id?: string;
      arguments?: unknown;
      result?: unknown;
    }) => {
      if (
        payload.conversation_id !== conversationId
        || (delegatedAgentRunId && payload.run_id !== delegatedAgentRunId)
      ) {
        return;
      }
      const toolCall = liveToolCallFromPayload(payload);
      if (!toolCall) return;
      const resultToolId = payload.tool_name ? parseToolResultId(payload.tool_name) : undefined;
      const targetId = resultToolId ?? toolCall.id;
      setLiveToolCalls((current) => {
        const existingIndex = current.findIndex((entry) => entry.id === targetId);
        if (existingIndex < 0) return resultToolId ? current : [...current, toolCall];
        const next = [...current];
        const existing = next[existingIndex]!;
        next[existingIndex] = {
          ...existing,
          ...(resultToolId ? {} : { name: toolCall.name, arguments: toolCall.arguments }),
          ...(toolCall.result != null ? { result: toolCall.result } : {}),
        };
        return next;
      });
    };

    const updateLiveTask = (payload: {
      conversation_id?: string;
      run_id?: string;
      tool_use_id?: string;
      description?: string;
      subagent_type?: string;
      model?: string;
      status?: string;
      text_output?: string;
      total_tokens?: number;
      total_tool_uses?: number;
      total_tool_use_count?: number;
      duration_ms?: number;
      total_duration_ms?: number;
      delegated_job_id?: string;
    }) => {
      if (
        payload.conversation_id !== conversationId
        || (delegatedAgentRunId && payload.run_id !== delegatedAgentRunId)
      ) {
        return;
      }
      const toolCall = liveTaskToolCallFromPayload({
        ...payload,
        total_tool_uses: payload.total_tool_uses ?? payload.total_tool_use_count,
        duration_ms: payload.duration_ms ?? payload.total_duration_ms,
      });
      if (!toolCall) return;
      setLiveToolCalls((current) => mergeLiveToolCalls([...current, toolCall]));
    };

    const unsubscribers = [
      bus.subscribe<{ conversation_id?: string; run_id?: string; text?: string }>(
        "agent:chunk",
        appendLiveChunk,
      ),
      bus.subscribe("agent:tool_call", updateLiveToolCall),
      bus.subscribe("agent:task_started", updateLiveTask),
      bus.subscribe("agent:task_completed", updateLiveTask),
      bus.subscribe<{
        conversation_id?: string;
        message_id?: string;
        role?: string;
      }>("agent:message_created", invalidateTranscript),
      bus.subscribe<{ conversation_id?: string }>("agent:run_completed", invalidateTranscript),
      bus.subscribe<{ conversation_id?: string }>("agent:error", invalidateTranscript),
    ];

    return () => {
      unsubscribers.forEach((unsubscribe) => unsubscribe());
    };
  }, [bus, conversationId, delegatedAgentRunId, queryClient]);

  const entries = buildTaskCardTranscriptEntriesFromConversation(messages);
  const liveTextHasPersisted = finalizedLiveMessageId
    ? entries.some((entry) => entry.id === finalizedLiveMessageId)
    : false;
  const showLiveText = liveText.trim().length > 0 && !liveTextHasPersisted;
  const persistedToolIds = new Set(
    entries.flatMap((entry) => entry.blocks.flatMap((block) => (
      block.type === "tool_call" ? [block.toolCall.id] : []
    ))),
  );
  const visibleLiveToolCalls = liveToolCalls.filter(
    (toolCall) => !persistedToolIds.has(toolCall.id),
  );
  const liveEntry = buildTaskCardTranscriptEntryFromToolCall({
    entryId: `delegated-live:${delegatedAgentRunId ?? conversationId}`,
    bodyText: showLiveText ? liveText : undefined,
    childToolCalls: visibleLiveToolCalls,
  });

  if (entries.length > 0 || liveEntry.blocks.length > 0) {
    return (
      <div className="space-y-3">
        {entries.length > 0 && (
          <>
            <div
              className="text-[0.625rem] uppercase tracking-[0.08em]"
              style={{ color: "var(--text-muted)" }}
            >
              Delegated conversation
            </div>
            <TaskCardTranscriptView
              entries={entries}
              dataTestId="delegated-conversation-transcript"
            />
          </>
        )}
        {liveEntry.blocks.length > 0 && (
          <TaskCardTranscriptView entries={[liveEntry]} dataTestId="delegated-live-transcript" />
        )}
      </div>
    );
  }

  if (delegatedConversation.isLoading || !activeStateSettled) {
    return (
      <div
        className="text-[0.6875rem] px-2 py-1.5 rounded"
        style={{
          backgroundColor: "var(--bg-surface)",
          color: "var(--text-muted)",
        }}
      >
        Loading delegated conversation...
      </div>
    );
  }

  if (delegatedConversation.isError || activeStateError) {
    return fallbackText ? (
      <FallbackText text={fallbackText} />
    ) : (
      <div
        className="text-[0.6875rem] px-2 py-1.5 rounded"
        style={{
          backgroundColor: "var(--status-error-muted)",
          color: "var(--status-error)",
        }}
      >
        {activeStateError
          ? "Unable to recover the delegated live state."
          : "Unable to load delegated conversation."}
      </div>
    );
  }

  if (fallbackText) {
    return <FallbackText text={fallbackText} />;
  }

  return (
    <div
      className="text-[0.6875rem] px-2 py-1.5 rounded"
      style={{
        backgroundColor: "var(--bg-surface)",
        color: "var(--text-muted)",
      }}
    >
      {isChildActive ? "Waiting for delegated output..." : "No delegated output available."}
    </div>
  );
}
