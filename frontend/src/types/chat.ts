// Chat context types and Zod schemas
// Types for context-aware chat panel behavior

import { z } from "zod";

import { ContextTypeSchema } from "@/types/chat-conversation";
import { APP_VIEW_VALUES, type AppView } from "@/types/app-view";

// ============================================================================
// View Types
// ============================================================================

/**
 * View type values for chat context
 */
export const CHAT_CONTEXT_VIEW_VALUES = [
  ...APP_VIEW_VALUES,
  "kanban",
  "graph",
  "ideation",
  "task_detail",
] as const;

export const ChatContextViewSchema = z.enum(CHAT_CONTEXT_VIEW_VALUES);
export type ChatContextView = z.infer<typeof ChatContextViewSchema>;

// ============================================================================
// Chat Context
// ============================================================================

/**
 * Chat context schema - describes the current state of the UI
 * The chat panel adapts its behavior based on this context
 */
export const ChatContextSchema = z.object({
  /** Current view being displayed */
  view: ChatContextViewSchema,
  /** Current project ID */
  projectId: z.string().min(1),
  /** Explicit backend conversation context for non-project Agents hosts. */
  contextTypeOverride: ContextTypeSchema.optional(),
  contextIdOverride: z.string().min(1).optional(),
  /** Selected task ID (for kanban with selection or task_detail view) */
  selectedTaskId: z.string().optional(),
  /** Current ideation session ID (for ideation view) */
  ideationSessionId: z.string().optional(),
});

export type ChatContext = z.infer<typeof ChatContextSchema>;

// ============================================================================
// Type Guards
// ============================================================================

/**
 * Check if context is in kanban view
 */
export function isKanbanContext(context: ChatContext): boolean {
  return context.view === "kanban";
}

/**
 * Check if context is in ideation view
 */
export function isIdeationContext(context: ChatContext): boolean {
  return context.view === "ideation";
}

/**
 * Check if context is in task detail view
 */
export function isTaskDetailContext(context: ChatContext): boolean {
  return context.view === "task_detail";
}

/**
 * Check if context is in activity view
 */
export function isActivityContext(context: ChatContext): boolean {
  return context.view === "activity";
}

/**
 * Check if context is in ticketing view
 */
export function isTicketingContext(context: ChatContext): boolean {
  return context.view === "ticketing";
}

/**
 * Check if context is in GitHub view
 */
export function isGitHubContext(context: ChatContext): boolean {
  return context.view === "github";
}

/**
 * Check if context is in Granola view
 */
export function isGranolaContext(context: ChatContext): boolean {
  return context.view === "granola";
}

/**
 * Check if context has a selected task
 */
export function hasSelectedTask(context: ChatContext): boolean {
  return context.selectedTaskId !== undefined;
}

/**
 * Check if context has an active ideation session
 */
export function hasIdeationSession(context: ChatContext): boolean {
  return context.ideationSessionId !== undefined;
}

// ============================================================================
// Factory Functions
// ============================================================================

/**
 * Create a kanban view context
 */
export function createKanbanContext(
  projectId: string,
  selectedTaskId?: string
): ChatContext {
  return {
    view: "kanban",
    projectId,
    selectedTaskId,
  };
}

/**
 * Create an ideation view context
 */
export function createIdeationContext(
  projectId: string,
  ideationSessionId: string
): ChatContext {
  return {
    view: "ideation",
    projectId,
    ideationSessionId,
  };
}

/**
 * Create a task detail view context
 */
export function createTaskDetailContext(
  projectId: string,
  selectedTaskId: string
): ChatContext {
  return {
    view: "task_detail",
    projectId,
    selectedTaskId,
  };
}

/**
 * Create a simple project context (e.g. activity view)
 */
export function createProjectContext(
  projectId: string,
  view: AppView
): ChatContext {
  return {
    view,
    projectId,
  };
}

// ============================================================================
// Review Chat Context
// ============================================================================

/**
 * Review chat context - used for live chat with AI reviewer
 */
export interface ReviewChatContext {
  type: 'review';
  taskId: string;
  reviewId: string;
}
