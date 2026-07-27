import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type { ChatConversation } from "@/types/chat-conversation";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useChildSessionStatus } from "@/hooks/useChildSessionStatus";
import { chatKeys, useConversationHistoryWindow } from "@/hooks/useChat";
import { cn } from "@/lib/utils";
import { MessageItem } from "./MessageItem";

interface ChildSessionTranscriptModalProps {
  sessionId: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

function mostRecentConversation(conversations: ChatConversation[]): ChatConversation | null {
  return conversations.reduce<ChatConversation | null>((current, candidate) => {
    if (!current) {
      return candidate;
    }
    const currentTime = Date.parse(current.lastMessageAt ?? current.updatedAt ?? current.createdAt);
    const candidateTime = Date.parse(candidate.lastMessageAt ?? candidate.updatedAt ?? candidate.createdAt);
    return candidateTime > currentTime ? candidate : current;
  }, null);
}

function statusLabel(status: string | undefined): string {
  if (status === "likely_generating") {
    return "Running";
  }
  if (status === "likely_waiting") {
    return "Waiting";
  }
  return "Idle";
}

export function ChildSessionTranscriptModal({
  sessionId,
  open,
  onOpenChange,
}: ChildSessionTranscriptModalProps) {
  const statusQuery = useChildSessionStatus(open ? sessionId : undefined);
  const conversationsQuery = useQuery({
    queryKey: sessionId
      ? chatKeys.conversationList("ideation", sessionId)
      : chatKeys.conversationList("ideation", ""),
    queryFn: () => chatApi.listConversations("ideation", sessionId!, true),
    enabled: open && !!sessionId,
    staleTime: 2_000,
    refetchInterval: open ? 5_000 : false,
  });
  const conversation = useMemo(
    () => mostRecentConversation(conversationsQuery.data ?? []),
    [conversationsQuery.data],
  );
  const conversationQuery = useConversationHistoryWindow(conversation?.id ?? null, {
    enabled: open && !!conversation?.id,
    pageSize: 40,
  });

  const title = statusQuery.data?.title ?? conversation?.title ?? "Ideation run";
  const status = statusLabel(statusQuery.data?.agent_state.estimated_status);
  const messages = conversationQuery.data?.messages ?? [];
  const isLoading =
    conversationsQuery.isLoading ||
    (Boolean(conversation?.id) && conversationQuery.isLoading);
  const error = conversationsQuery.error ?? conversationQuery.error;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-w-5xl h-[82vh] grid-rows-[auto_1fr] p-0 overflow-hidden"
        overlayClassName="bg-[var(--overlay-scrim-deep)] backdrop-blur-[10px]"
        hideCloseButton={false}
      >
        <DialogHeader className="px-5 py-4">
          <div className="min-w-0">
            <DialogTitle className="truncate text-base">{title}</DialogTitle>
            <DialogDescription className="mt-1 flex items-center gap-2">
              <span>Ideation conversation</span>
              <span
                className={cn(
                  "rounded-full px-2 py-0.5 text-[0.6875rem] font-medium",
                  status === "Running"
                    ? "bg-[var(--status-success-muted)] text-[var(--status-success)]"
                    : "bg-[var(--bg-hover)] text-[var(--text-muted)]",
                )}
              >
                {status}
              </span>
            </DialogDescription>
          </div>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          {isLoading ? (
            <div className="text-sm text-[var(--text-muted)]">Loading conversation...</div>
          ) : error ? (
            <div className="text-sm text-[var(--status-error)]">Unable to load this ideation run.</div>
          ) : messages.length === 0 ? (
            <div className="text-sm text-[var(--text-muted)]">No messages yet.</div>
          ) : (
            <div className="space-y-2">
              {messages.map((message, index) => (
                <MessageItem
                  key={message.id}
                  role={message.role}
                  content={message.content}
                  createdAt={message.createdAt}
                  isLastInList={index === messages.length - 1}
                  toolCalls={message.toolCalls}
                  contentBlocks={message.contentBlocks}
                  providerHarness={message.providerHarness}
                  providerSessionId={message.providerSessionId}
                  upstreamProvider={message.upstreamProvider}
                  providerProfile={message.providerProfile}
                  logicalModel={message.logicalModel}
                  effectiveModelId={message.effectiveModelId}
                  logicalEffort={message.logicalEffort}
                  effectiveEffort={message.effectiveEffort}
                  inputTokens={message.inputTokens}
                  outputTokens={message.outputTokens}
                  cacheCreationTokens={message.cacheCreationTokens}
                  cacheReadTokens={message.cacheReadTokens}
                  estimatedUsd={message.estimatedUsd}
                />
              ))}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
