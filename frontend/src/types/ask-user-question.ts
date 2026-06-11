// AskUserQuestion types and Zod schemas
// For handling agent questions that require user input during execution

import { z } from "zod";

// ========================================
// AskUserQuestion Option
// ========================================

/**
 * An option for the user to select when answering a question
 */
export const AskUserQuestionOptionSchema = z.object({
  label: z.string().min(1),
  value: z.string().optional().nullable(),
  description: z.string().optional().nullable(),
});

export type AskUserQuestionOption = z.infer<typeof AskUserQuestionOptionSchema>;

// ========================================
// AskUserQuestion Payload
// ========================================

/**
 * Payload sent from agent when it needs user input
 * Rendered as an interactive question component in the UI
 */
export const AskUserQuestionPayloadSchema = z.object({
  requestId: z.string().min(1),
  taskId: z.string().min(1).optional(),
  sessionId: z.string().min(1).optional(),
  question: z.string().min(1),
  header: z.string().optional().nullable(),
  options: z.array(AskUserQuestionOptionSchema).default([]),
  multiSelect: z.boolean().default(false),
  allowSkip: z.boolean().optional(),
  batchIndex: z.number().int().positive().optional().nullable(),
  batchTotal: z.number().int().positive().optional().nullable(),
  metadata: z.record(z.string(), z.unknown()).optional().nullable(),
});

export type AskUserQuestionPayload = z.infer<typeof AskUserQuestionPayloadSchema>;

// ========================================
// AskUserQuestion Response
// ========================================

/**
 * Response sent back to agent with user's answer
 */
export const AskUserQuestionResponseSchema = z.object({
  requestId: z.string().min(1).optional(),
  taskId: z.string().min(1).optional(),
  selectedOptions: z.array(z.string()),
  customResponse: z.string().optional(),
  skipped: z.boolean().optional(),
});

export type AskUserQuestionResponse = z.infer<typeof AskUserQuestionResponseSchema>;

// ========================================
// List Schemas
// ========================================

/**
 * Schema for a list of question payloads
 */
export const AskUserQuestionPayloadListSchema = z.array(AskUserQuestionPayloadSchema);
export type AskUserQuestionPayloadList = z.infer<typeof AskUserQuestionPayloadListSchema>;

// ========================================
// Helper Functions
// ========================================

/**
 * Check if the user selected at least one option
 */
export function hasSelection(response: AskUserQuestionResponse): boolean {
  return response.selectedOptions.length > 0;
}

/**
 * Check if the user provided a custom response
 */
export function hasCustomResponse(response: AskUserQuestionResponse): boolean {
  return response.customResponse !== undefined && response.customResponse.length > 0;
}

/**
 * Check if the response is valid (has selection or custom response)
 */
export function isValidResponse(response: AskUserQuestionResponse): boolean {
  return response.skipped === true || hasSelection(response) || hasCustomResponse(response);
}

/**
 * Create a response with a single selected option
 */
export function createSingleSelectResponse(
  selectedOption: string,
  ids: { requestId?: string; taskId?: string }
): AskUserQuestionResponse {
  return {
    ...ids,
    selectedOptions: [selectedOption],
  };
}

/**
 * Create a response with multiple selected options
 */
export function createMultiSelectResponse(
  selectedOptions: string[],
  ids: { requestId?: string; taskId?: string }
): AskUserQuestionResponse {
  return {
    ...ids,
    selectedOptions,
  };
}

/**
 * Create a response with a custom text response
 */
export function createCustomResponse(
  customResponse: string,
  ids: { requestId?: string; taskId?: string }
): AskUserQuestionResponse {
  return {
    ...ids,
    selectedOptions: [],
    customResponse,
  };
}

/**
 * Create a response that explicitly skips a question
 */
export function createSkippedResponse(
  ids: { requestId?: string; taskId?: string }
): AskUserQuestionResponse {
  return {
    ...ids,
    selectedOptions: [],
    skipped: true,
  };
}
