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

  const candidates: SessionCandidate[] = [];
  for (const message of [...messages].reverse()) {
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
      candidates.push(...extractAttachedSessionCandidates(toolCall));
    }
  }

  const bestCandidate = candidates
    .filter((candidate) => candidate.sessionId !== fallbackSessionId)
    .sort((a, b) => b.score - a.score)[0];
  if (bestCandidate && bestCandidate.score > 0) {
    return bestCandidate.sessionId;
  }

  const firstCandidate = candidates[0];
  if (firstCandidate?.sessionId) {
    return firstCandidate.sessionId;
  }

  return fallbackSessionId ?? null;
}

interface SessionCandidate {
  sessionId: string;
  score: number;
}

function extractAttachedSessionId(toolCall: ToolCall): string | null {
  return extractAttachedSessionCandidates(toolCall)[0]?.sessionId ?? null;
}

function extractAttachedSessionCandidates(toolCall: ToolCall): SessionCandidate[] {
  const name = toolCall.name.toLowerCase();
  if (
    !name.includes("start_ideation_session") &&
    !name.includes("v1_start_ideation") &&
    !name.includes("v1_send_ideation_message") &&
    !name.includes("v1_get_ideation_status") &&
    !name.includes("v1_list_ideation_sessions") &&
    !name.includes("v1_list_proposals") &&
    !name.includes("v1_get_session_tasks") &&
    !name.includes("create_child_session") &&
    !name.includes("create_plan_artifact") &&
    !name.includes("update_plan_artifact") &&
    !name.includes("edit_plan_artifact") &&
    !name.includes("get_session_plan")
  ) {
    return [];
  }
  return [
    ...extractSessionCandidatesFromValue(toolCall.result),
    ...extractSessionCandidatesFromValue(toolCall.arguments),
  ];
}

function extractSessionIdFromValue(value: unknown): string | null {
  return extractSessionCandidatesFromValue(value)[0]?.sessionId ?? null;
}

function extractSessionCandidatesFromValue(value: unknown): SessionCandidate[] {
  const parsed = parseMcpToolResultRaw(value);
  if (parsed !== null) {
    const parsedCandidates = extractSessionCandidatesFromParsedValue(parsed);
    if (parsedCandidates.length > 0) {
      return parsedCandidates;
    }
  }
  return extractSessionCandidatesFromParsedValue(value);
}

function extractSessionCandidatesFromParsedValue(value: unknown): SessionCandidate[] {
  if (!value) {
    return [];
  }
  if (Array.isArray(value)) {
    const candidates: SessionCandidate[] = [];
    for (const item of value) {
      candidates.push(...extractSessionCandidatesFromValue(item));
    }
    return candidates;
  }
  if (typeof value === "object") {
    const record = value as Record<string, unknown>;
    const candidates: SessionCandidate[] = [];
    if (typeof record.session_id === "string") {
      candidates.push({ sessionId: record.session_id, score: scoreSessionRecord(record) });
    }
    if (typeof record.sessionId === "string") {
      candidates.push({ sessionId: record.sessionId, score: scoreSessionRecord(record) });
    }
    if (typeof record.child_session_id === "string") {
      candidates.push({ sessionId: record.child_session_id, score: 0 });
    }
    if (typeof record.childSessionId === "string") {
      candidates.push({ sessionId: record.childSessionId, score: 0 });
    }
    if (typeof record.id === "string" && looksLikeIdeationSessionRecord(record)) {
      candidates.push({ sessionId: record.id, score: scoreSessionRecord(record) });
    }
    for (const nestedKey of [
      "result",
      "data",
      "session",
      "ideation_session",
      "structured_content",
      "structuredContent",
      "content",
      "sessions",
      "proposals",
      "tasks",
    ]) {
      candidates.push(...extractSessionCandidatesFromValue(record[nestedKey]));
    }
    if (typeof record.text === "string") {
      try {
        candidates.push(...extractSessionCandidatesFromValue(JSON.parse(record.text)));
      } catch {
        const textSession = extractSessionIdFromText(record.text);
        if (textSession) {
          candidates.push({ sessionId: textSession, score: 1 });
        }
      }
    }
    return dedupeCandidates(candidates);
  }
  if (typeof value === "string") {
    const textSession = extractSessionIdFromText(value);
    return textSession ? [{ sessionId: textSession, score: 1 }] : [];
  }
  return [];
}

function looksLikeIdeationSessionRecord(record: Record<string, unknown>): boolean {
  return (
    "proposal_count" in record ||
    "proposalCount" in record ||
    "plan_artifact_id" in record ||
    "planArtifactId" in record ||
    "verification_status" in record ||
    "verificationStatus" in record ||
    "delivery_status" in record ||
    "deliveryStatus" in record
  );
}

function scoreSessionRecord(record: Record<string, unknown>): number {
  let score = 1;
  const proposalCount =
    numericValue(record.proposal_count) ??
    numericValue(record.proposalCount) ??
    numericValue(record.count);
  if ((proposalCount ?? 0) > 0) {
    score += 50 + Math.min(proposalCount ?? 0, 20);
  }
  if (stringValue(record.status) === "accepted") {
    score += 40;
  }
  if (stringValue(record.acceptance_status) === "accepted") {
    score += 40;
  }
  if (stringValue(record.plan_artifact_id) || stringValue(record.planArtifactId)) {
    score += 30;
  }
  if (stringValue(record.delivery_status) || stringValue(record.deliveryStatus)) {
    score += 10;
  }
  return score;
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function numericValue(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function extractSessionIdFromText(text: string): string | null {
  return (
    text.match(
      /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/i,
    )?.[0] ?? null
  );
}

function dedupeCandidates(candidates: SessionCandidate[]): SessionCandidate[] {
  const bySession = new Map<string, SessionCandidate>();
  for (const candidate of candidates) {
    const existing = bySession.get(candidate.sessionId);
    if (!existing || candidate.score > existing.score) {
      bySession.set(candidate.sessionId, candidate);
    }
  }
  return [...bySession.values()];
}
