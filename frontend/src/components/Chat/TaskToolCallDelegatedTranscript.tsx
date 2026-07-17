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
import { buildTaskCardTranscriptEntriesFromConversation } from "./TaskCardTranscript.utils";

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
  fallbackText,
}: {
  conversationId: string;
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
  const [isChildActive, setIsChildActive] = useState(false);

  useEffect(() => {
    let cancelled = false;
    liveTextRef.current = "";
    setLiveText("");
    setFinalizedLiveMessageId(null);
    setActiveStateSettled(false);
    setIsChildActive(false);

    void chatApi
      .getConversationActiveState(conversationId)
      .then((activeState) => {
        if (cancelled) return;
        setIsChildActive(activeState.is_active);
        if (activeState.partial_text.trim().length > 0) {
          setLiveText((current) => {
            const next = mergeStreamingTextSnapshot(activeState.partial_text, current);
            liveTextRef.current = next;
            return next;
          });
        }
      })
      .catch(() => {
        // Best-effort recovery only. Persisted history and live events remain authoritative.
      })
      .finally(() => {
        if (!cancelled) {
          setActiveStateSettled(true);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [conversationId]);

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

    const appendLiveChunk = (payload: { conversation_id?: string; text?: string }) => {
      if (payload.conversation_id !== conversationId || !payload.text) {
        return;
      }
      setIsChildActive(true);
      setLiveText((current) => {
        const next = mergeStreamingTextSnapshot(current, payload.text ?? "");
        liveTextRef.current = next;
        return next;
      });
    };

    const unsubscribers = [
      bus.subscribe<{ conversation_id?: string; text?: string }>("agent:chunk", appendLiveChunk),
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
  }, [bus, conversationId, queryClient]);

  const entries = buildTaskCardTranscriptEntriesFromConversation(messages);
  const liveTextHasPersisted = finalizedLiveMessageId
    ? entries.some((entry) => entry.id === finalizedLiveMessageId)
    : false;
  const showLiveText = liveText.trim().length > 0 && !liveTextHasPersisted;

  if (entries.length > 0 || showLiveText) {
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
        {showLiveText && <FallbackText text={liveText} />}
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

  if (delegatedConversation.isError) {
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
        Unable to load delegated conversation.
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
