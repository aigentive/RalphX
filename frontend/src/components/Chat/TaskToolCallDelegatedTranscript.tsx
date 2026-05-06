import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
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

  useEffect(() => {
    const invalidateTranscript = (payload: { conversation_id?: string }) => {
      if (payload.conversation_id !== conversationId) {
        return;
      }
      invalidateConversationDataQueries(queryClient, conversationId);
    };

    const unsubscribers = [
      bus.subscribe<{ conversation_id?: string }>("agent:message_created", invalidateTranscript),
      bus.subscribe<{ conversation_id?: string }>("agent:run_completed", invalidateTranscript),
      bus.subscribe<{ conversation_id?: string }>("agent:error", invalidateTranscript),
    ];

    return () => {
      unsubscribers.forEach((unsubscribe) => unsubscribe());
    };
  }, [bus, conversationId, queryClient]);

  if (delegatedConversation.isLoading) {
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

  const entries = buildTaskCardTranscriptEntriesFromConversation(messages);

  if (entries.length === 0) {
    return fallbackText ? <FallbackText text={fallbackText} /> : null;
  }

  return (
    <div className="space-y-3">
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
    </div>
  );
}
