/**
 * Chat Context Registry — Single source of truth for all context-related derivations
 *
 * Replaces 4 duplicated `buildContextKey` implementations and 5+ scattered
 * context type resolution ternary chains with a centralized registry.
 *
 * Usage:
 *   import { buildStoreKey, resolveContextType, getContextConfig } from "@/lib/chat-context-registry";
 */

import type { ContextType } from "@/types/chat-conversation";
import type { InternalStatus } from "@/types/status";
import {
  EXECUTION_STATUSES,
  ALL_REVIEW_STATUSES,
  MERGE_STATUSES,
} from "@/types/status";

// ============================================================================
// Registry Types
// ============================================================================

export interface ChatContextConfig {
  /** Store key prefix for queue/agent state (e.g., "session", "task_execution") */
  storeKeyPrefix: string;
  /** Input placeholder text */
  placeholder: string;
  /** Label for context indicator */
  label: string;
  /** Agent type for StatusActivityBadge */
  agentType: "worker" | "reviewer" | "merger" | "ideation" | "chat" | null;
  /** Feature flags */
  supportsStreamingText: boolean;
  supportsSubagentTasks: boolean;
  supportsDiffViews: boolean;
  supportsHookEvents: boolean;
  supportsQueue: boolean;
  /** Whether this context can have an agent team */
  supportsTeamMode: boolean;
  /** Where to show the team activity panel (null = not supported) */
  teamActivityPanelPosition: "right" | "bottom" | null;
}

// ============================================================================
// Registry
// ============================================================================

export const CHAT_CONTEXT_REGISTRY: Record<ContextType, ChatContextConfig> = {
  ideation: {
    storeKeyPrefix: "session",
    placeholder: "Send a message...",
    label: "Ideation",
    agentType: "ideation",
    supportsStreamingText: true,
    supportsSubagentTasks: true,
    supportsDiffViews: false,
    supportsHookEvents: false,
    supportsQueue: true,
    supportsTeamMode: true,
    teamActivityPanelPosition: "right",
  },
  task: {
    storeKeyPrefix: "task",
    placeholder: "Ask about this task...",
    label: "Task",
    agentType: "chat",
    supportsStreamingText: true,
    supportsSubagentTasks: false,
    supportsDiffViews: false,
    supportsHookEvents: false,
    supportsQueue: true,
    supportsTeamMode: false,
    teamActivityPanelPosition: null,
  },
  project: {
    storeKeyPrefix: "project",
    placeholder: "Send a message...",
    label: "Project",
    agentType: "chat",
    supportsStreamingText: true,
    supportsSubagentTasks: false,
    supportsDiffViews: false,
    supportsHookEvents: false,
    supportsQueue: true,
    supportsTeamMode: false,
    teamActivityPanelPosition: null,
  },
  task_execution: {
    storeKeyPrefix: "task_execution",
    placeholder: "Message worker...",
    label: "Execution",
    agentType: "worker",
    supportsStreamingText: true,
    supportsSubagentTasks: true,
    supportsDiffViews: true,
    supportsHookEvents: true,
    supportsQueue: true,
    supportsTeamMode: true,
    teamActivityPanelPosition: "bottom",
  },
  review: {
    storeKeyPrefix: "review",
    placeholder: "Message reviewer...",
    label: "Review",
    agentType: "reviewer",
    supportsStreamingText: true,
    supportsSubagentTasks: true,
    supportsDiffViews: true,
    supportsHookEvents: false,
    supportsQueue: true,
    supportsTeamMode: false,
    teamActivityPanelPosition: null,
  },
  merge: {
    storeKeyPrefix: "merge",
    placeholder: "Message merger...",
    label: "Merge",
    agentType: "merger",
    supportsStreamingText: true,
    supportsSubagentTasks: true,
    supportsDiffViews: true,
    supportsHookEvents: false,
    supportsQueue: true,
    supportsTeamMode: false,
    teamActivityPanelPosition: null,
  },
  delegation: {
    storeKeyPrefix: "delegation",
    placeholder: "Message delegated agent...",
    label: "Delegation",
    agentType: "chat",
    supportsStreamingText: true,
    supportsSubagentTasks: true,
    supportsDiffViews: false,
    supportsHookEvents: false,
    supportsQueue: true,
    supportsTeamMode: false,
    teamActivityPanelPosition: null,
  },
};

// ============================================================================
// Store Key Builder + Parser
// ============================================================================

/**
 * Build a store context key from context type and ID.
 *
 * Replaces 4 duplicated implementations:
 * - `buildContextKey()` in useChat.ts
 * - `buildContextKey()` in useAgentEvents.ts
 * - `buildTaskContextKey()` in useTaskChat.ts
 * - `getContextKey()` in chatStore.ts
 *
 * Format: `${storeKeyPrefix}:${contextId}`
 */
export function buildStoreKey(contextType: ContextType, contextId: string): string {
  const config = CHAT_CONTEXT_REGISTRY[contextType];
  return `${config.storeKeyPrefix}:${contextId}`;
}

/**
 * Reverse map from storeKeyPrefix → ContextType.
 * Derived from CHAT_CONTEXT_REGISTRY to stay in sync automatically.
 */
const REVERSE_PREFIX_MAP: Record<string, ContextType> = Object.fromEntries(
  (Object.entries(CHAT_CONTEXT_REGISTRY) as [ContextType, { storeKeyPrefix: string }][]).map(
    ([contextType, config]) => [config.storeKeyPrefix, contextType],
  ),
);

/**
 * Parse a store key back into its contextType and contextId components.
 * Reverse of buildStoreKey.
 *
 * @returns { contextType, contextId } or null if the key is not recognized.
 */
export function parseStoreKey(
  key: string,
): { contextType: ContextType; contextId: string } | null {
  const colonIndex = key.indexOf(":");
  if (colonIndex === -1) return null;
  const prefix = key.slice(0, colonIndex);
  const contextId = key.slice(colonIndex + 1);
  const contextType = REVERSE_PREFIX_MAP[prefix];
  if (!contextType || !contextId) return null;
  return { contextType, contextId };
}

// ============================================================================
// Context Type Resolution
// ============================================================================

/**
 * Resolve the ContextType from task internal status, ideation session, and task presence.
 *
 * Replaces 5+ scattered ternary chains in:
 * - IntegratedChatPanel.tsx (mode computation)
 * - useChatPanelContext.ts (contextKey + currentContextType)
 * - useIntegratedChatHandlers.ts (getContextForMode)
 * - useChatPanelHandlers.ts
 * - Agents and ideation chat hosts
 *
 * @param internalStatus - Task's current internal status (or effective status in history mode)
 * @param ideationSessionId - If truthy, ideation context wins
 * @param taskId - If truthy (and no ideation), task-related context
 * @returns The resolved ContextType
 */
export function resolveContextType(
  internalStatus: InternalStatus | string | undefined,
  ideationSessionId: string | undefined,
  taskId: string | undefined,
): ContextType {
  // Ideation always wins
  if (ideationSessionId) {
    return "ideation";
  }

  // Task-related contexts — check status to determine specific type
  if (taskId && internalStatus) {
    if (internalStatus === "waiting_on_pr") {
      return "task";
    }
    if ((MERGE_STATUSES as readonly string[]).includes(internalStatus)) {
      return "merge";
    }
    if ((EXECUTION_STATUSES as readonly string[]).includes(internalStatus)) {
      return "task_execution";
    }
    if (
      (ALL_REVIEW_STATUSES as readonly string[]).includes(internalStatus) ||
      internalStatus === "approved"
    ) {
      return "review";
    }
    // Non-agent task statuses (backlog, ready, blocked, etc.)
    return "task";
  }

  // Task selected but no status info → generic task chat
  if (taskId) {
    return "task";
  }

  // Fallback: project-level chat
  return "project";
}

// ============================================================================
// Config Lookup
// ============================================================================

/**
 * Get the context configuration for a given context type.
 */
export function getContextConfig(contextType: ContextType): ChatContextConfig {
  return CHAT_CONTEXT_REGISTRY[contextType];
}

// ============================================================================
// Convenience: Check if a status maps to an agent context
// ============================================================================

/**
 * Returns true if the resolved context type has an agent (execution/review/merge).
 * Useful for determining if the chat panel should show agent-specific UI.
 */
export function isAgentContext(contextType: ContextType): boolean {
  return contextType === "task_execution" || contextType === "review" || contextType === "merge";
}
