/**
 * Ask User Question API Module
 *
 * Provides a centralized API wrapper for answering agent questions.
 * This module follows the domain API pattern used by other centralized modules.
 */

import { invoke } from "@tauri-apps/api/core";
import type { AskUserQuestionPayload, AskUserQuestionResponse } from "@/types/ask-user-question";

// ============================================================================
// Ask User Question API Object
// ============================================================================

/**
 * Ask User Question API object containing typed Tauri command wrappers
 */
export interface ResolveQuestionInput {
  requestId: string;
  selectedOptions: string[];
  customResponse?: string;
  skipped?: boolean;
}

export interface ResolveQuestionResult {
  success: boolean;
  message?: string | null;
  deliveredToWaitingAgent: boolean;
  planModeProposalHandled?: boolean;
}

/** Raw shape returned by the backend get_pending_questions command (snake_case) */
interface PendingQuestionInfoRaw {
  request_id: string;
  session_id: string;
  question: string;
  header?: string | null;
  options: Array<{ value: string; label: string; description?: string }>;
  multi_select: boolean;
  allow_skip?: boolean | null;
  batch_index?: number | null;
  batch_total?: number | null;
  metadata?: Record<string, unknown> | null;
}

export const askUserQuestionApi = {
  /**
   * Submit an answer to an agent's question (legacy task-based flow)
   * @param response The user's response including selected options
   */
  answerQuestion: async (response: AskUserQuestionResponse): Promise<void> => {
    await invoke("answer_user_question", {
      input: {
        taskId: response.taskId,
        selectedOptions: response.selectedOptions,
        customResponse: response.customResponse,
      },
    });
  },

  /**
   * Resolve an MCP-based question by requestId
   * Used when the agent asks questions via the ask_user_question MCP tool
   * @param input The resolution including requestId and selected options
   */
  resolveQuestion: async (input: ResolveQuestionInput): Promise<ResolveQuestionResult> => {
    return await invoke<ResolveQuestionResult>("resolve_user_question", {
      args: {
        requestId: input.requestId,
        selectedOptions: input.selectedOptions,
        customResponse: input.customResponse,
        skipped: input.skipped ?? false,
      },
    });
  },

  /**
   * Fetch all unresolved questions from backend state, including durable
   * questions whose agent-side wait has timed out.
   */
  getPendingQuestions: async (): Promise<AskUserQuestionPayload[]> => {
    return toQuestionPayloads(
      await invoke<PendingQuestionInfoRaw[]>("get_pending_questions")
    );
  },

  /**
   * The AUTHORITATIVE pending question set (P-21, PR 2.7-c).
   *
   * Strict facade command: an unreadable question state raises instead of answering
   * `[]`, because this list is used to DROP banners that are no longer pending and an
   * empty answer would silently clear every one of them.
   */
  listPendingQuestionGates: async (): Promise<AskUserQuestionPayload[]> => {
    return toQuestionPayloads(
      await invoke<PendingQuestionInfoRaw[]>("list_pending_question_gates")
    );
  },
} as const;

function toQuestionPayloads(
  raw: PendingQuestionInfoRaw[]
): AskUserQuestionPayload[] {
  return raw.map((item) => ({
      requestId: item.request_id,
      sessionId: item.session_id,
      question: item.question,
      header: item.header ?? null,
      options: item.options,
      multiSelect: item.multi_select,
      allowSkip: item.allow_skip ?? true,
      batchIndex: item.batch_index ?? null,
      batchTotal: item.batch_total ?? null,
    metadata: item.metadata ?? null,
  }));
}
