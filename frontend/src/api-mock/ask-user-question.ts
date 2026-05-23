/**
 * Mock Ask User Question API
 *
 * Provides mock implementation for ask-user-question operations.
 * Used for browser testing and visual regression testing.
 */

import type { AskUserQuestionResponse } from "@/types/ask-user-question";
import type { AskUserQuestionPayload } from "@/types/ask-user-question";
import type { ResolveQuestionInput, ResolveQuestionResult } from "@/api/ask-user-question";

/**
 * Mock Ask User Question API matching the real API interface
 */
export const mockAskUserQuestionApi = {
  answerQuestion: async (_response: AskUserQuestionResponse): Promise<void> => {
    console.log("[mock] answerQuestion called");
  },

  resolveQuestion: async (_input: ResolveQuestionInput): Promise<ResolveQuestionResult> => {
    console.log("[mock] resolveQuestion called");
    return { success: true, message: null, deliveredToWaitingAgent: true };
  },

  getPendingQuestions: async (): Promise<AskUserQuestionPayload[]> => {
    return [];
  },
} as const;
