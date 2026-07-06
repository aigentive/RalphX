import type { ChatMessageResponse } from "@/api/chat";
import type { ContentBlockItem } from "@/components/Chat/MessageItem";
import type { ToolCall } from "@/components/Chat/ToolCallIndicator";
import { parseMcpToolResultRaw } from "@/components/Chat/tool-widgets/shared.constants";
import type { AgentConversation } from "./agentConversations";

export function resolveAttachedIdeationSessionId(
  conversation: AgentConversation | null,
  messages: ChatMessageResponse[],
  fallbackSessionId?: string | null,
): string | null {
  if (!conversation) {
    return fallbackSessionId ?? null;
  }
  if (conversation.contextType === "ideation") {
    return conversation.contextId;
  }

  for (const message of [...messages].reverse()) {
    if (!fallbackSessionId) {
      const referencedSessionId = extractComposerReferencedSessionId(
        message.metadata,
      );
      if (referencedSessionId) {
        return referencedSessionId;
      }
    }

    const toolCalls = [
      ...(message.toolCalls ?? []),
      ...(message.contentBlocks ?? [])
        .filter((block): block is ContentBlockItem & { type: "tool_use" } => block.type === "tool_use")
        .map((block) => ({
          id: block.id ?? "",
          name: block.name ?? "",
          arguments: block.arguments,
          result: block.result,
        })),
    ];
    for (const toolCall of toolCalls.reverse()) {
      const sessionId = extractAttachedSessionId(toolCall);
      if (sessionId) {
        return sessionId;
      }
    }
  }

  return fallbackSessionId ?? null;
}

function extractComposerReferencedSessionId(
  metadata: string | null | undefined,
): string | null {
  if (!metadata) {
    return null;
  }
  try {
    const parsed = JSON.parse(metadata) as unknown;
    if (!parsed || typeof parsed !== "object") {
      return null;
    }
    const references = (parsed as Record<string, unknown>)
      .composer_artifact_references;
    if (!Array.isArray(references)) {
      return null;
    }
    for (const reference of [...references].reverse()) {
      if (!reference || typeof reference !== "object") {
        continue;
      }
      const record = reference as Record<string, unknown>;
      const kind =
        typeof record.kind === "string" ? record.kind.toLowerCase() : null;
      const sessionId = record.sessionId ?? record.session_id;
      if (
        (kind === null || kind === "plan") &&
        typeof sessionId === "string" &&
        sessionId.length > 0
      ) {
        return sessionId;
      }
    }
  } catch {
    return null;
  }
  return null;
}

function extractAttachedSessionId(toolCall: ToolCall): string | null {
  const name = toolCall.name.toLowerCase();
  if (
    !name.includes("start_ideation_session") &&
    !name.includes("v1_start_ideation") &&
    !name.includes("v1_send_ideation_message") &&
    !name.includes("create_child_session") &&
    !name.includes("create_plan_artifact") &&
    !name.includes("update_plan_artifact") &&
    !name.includes("edit_plan_artifact") &&
    !name.includes("get_session_plan")
  ) {
    return null;
  }
  return extractSessionIdFromValue(toolCall.result) ?? extractSessionIdFromValue(toolCall.arguments);
}

function extractSessionIdFromValue(value: unknown): string | null {
  const parsed = parseMcpToolResultRaw(value);
  if (parsed !== null) {
    const parsedSessionId = extractSessionIdFromParsedValue(parsed);
    if (parsedSessionId) {
      return parsedSessionId;
    }
  }
  return extractSessionIdFromParsedValue(value);
}

function extractSessionIdFromParsedValue(value: unknown): string | null {
  if (!value) {
    return null;
  }
  if (Array.isArray(value)) {
    for (const item of value) {
      const nested = extractSessionIdFromValue(item);
      if (nested) {
        return nested;
      }
    }
    return null;
  }
  if (typeof value === "object") {
    const record = value as Record<string, unknown>;
    if (typeof record.session_id === "string") {
      return record.session_id;
    }
    if (typeof record.sessionId === "string") {
      return record.sessionId;
    }
    if (typeof record.child_session_id === "string") {
      return record.child_session_id;
    }
    if (typeof record.childSessionId === "string") {
      return record.childSessionId;
    }
    for (const nestedKey of [
      "result",
      "data",
      "session",
      "ideation_session",
      "structured_content",
      "structuredContent",
      "content",
    ]) {
      const nested = extractSessionIdFromValue(record[nestedKey]);
      if (nested) {
        return nested;
      }
    }
    if (typeof record.text === "string") {
      try {
        return extractSessionIdFromValue(JSON.parse(record.text));
      } catch {
        return null;
      }
    }
  }
  return null;
}
