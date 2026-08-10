/**
 * useMessageAttachments — Fetch attachments for messages
 *
 * Fetches attachments for all messages in a conversation and returns a map
 * of message ID to attachments array.
 */

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  chatApi,
  type ChatAttachmentResponse,
  type RemoteChatAttachmentResponse,
} from "@/api/chat";
import type { ChatMessageData } from "@/components/Chat/ChatMessageList";
import type { MessageAttachment } from "@/components/Chat/MessageAttachments";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";

const MESSAGE_ATTACHMENTS_QUERY_VERSION = "preview-v2";

export interface MessageAttachmentsResult {
  attachments: Map<string, MessageAttachment[]>;
  unavailableMessageIds: Set<string>;
}

/**
 * Transform ChatAttachmentResponse from backend to MessageAttachment for UI
 */
function transformAttachment(
  attachment: ChatAttachmentResponse | RemoteChatAttachmentResponse,
): MessageAttachment {
  const base = {
    id: attachment.id,
    fileName: attachment.fileName,
    fileSize: attachment.fileSize,
    ...("filePath" in attachment && { filePath: attachment.filePath }),
  };

  // Only include optional properties when they have values
  return {
    ...base,
    ...(attachment.mimeType !== null && { mimeType: attachment.mimeType }),
  };
}

/**
 * Fetch attachments for all messages in a list
 *
 * @param messages - Array of chat messages
 * @param conversationId - Current conversation ID (used as cache key)
 * @returns Map of message ID to attachments array
 */
export function useMessageAttachments(
  messages: ChatMessageData[],
  conversationId: string | null,
  options: { enabled?: boolean; metadataReadAvailable?: boolean } = {}
) {
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const userMessageAttachmentTargets = useMemo(
    () =>
      messages
        .filter((msg) => msg.role === "user")
        .map((msg) => ({
          renderMessageId: msg.id,
          lookupMessageId:
            msg.timelineSequence != null && msg.parentMessageId
              ? msg.parentMessageId
              : msg.id,
        })),
    [messages]
  );
  const userMessageAttachmentKey = useMemo(
    () =>
      userMessageAttachmentTargets.map((target) => [
        target.renderMessageId,
        target.lookupMessageId,
      ]),
    [userMessageAttachmentTargets]
  );

  return useQuery({
    queryKey: [
      "message-attachments",
      conversationId,
      MESSAGE_ATTACHMENTS_QUERY_VERSION,
      isRemoteEnvironment ? "remote" : "local",
      options.metadataReadAvailable ?? true,
      userMessageAttachmentKey,
    ],
    queryFn: async () => {
      const attachmentsMap = new Map<string, MessageAttachment[]>();
      const unavailableMessageIds = new Set<string>();

      await Promise.all(
        userMessageAttachmentTargets.map(async (target) => {
          if (isRemoteEnvironment && options.metadataReadAvailable === false) {
            unavailableMessageIds.add(target.renderMessageId);
            return;
          }
          try {
            const attachments = isRemoteEnvironment
              ? await chatApi.listRemoteMessageAttachments(target.lookupMessageId)
              : await chatApi.listMessageAttachments(target.lookupMessageId);
            if (attachments.length > 0) {
              attachmentsMap.set(target.renderMessageId, attachments.map(transformAttachment));
            }
          } catch {
            unavailableMessageIds.add(target.renderMessageId);
          }
        })
      );

      return {
        attachments: attachmentsMap,
        unavailableMessageIds,
      } satisfies MessageAttachmentsResult;
    },
    enabled:
      !!conversationId &&
      userMessageAttachmentTargets.length > 0 &&
      (options.enabled ?? true),
    staleTime: 30000, // Cache for 30 seconds
  });
}
