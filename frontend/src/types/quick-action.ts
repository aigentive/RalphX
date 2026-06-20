/**
 * Quick Action types for the command palette
 *
 * Defines the extensible quick action system used in PlanQuickSwitcherPalette.
 * Actions follow a state machine flow: idle → confirming → creating → success.
 */

import type { LucideIcon } from "lucide-react";

/**
 * State of a quick action flow
 */
export type QuickActionFlowState = "idle" | "confirming" | "creating" | "success";

/**
 * Quick action interface
 *
 * Extensible action type for the command palette. Implementations can be
 * agent conversations, task creation, search-by-id, etc.
 */
export interface QuickAction {
  /** Unique identifier (e.g., "agent-conversation", "create-task") */
  id: string;
  /** Display label (e.g., "Start new agent conversation") */
  label: string;
  /** Icon from lucide-react */
  icon: LucideIcon;
  /** Description generator based on query (e.g., `"${query}"`) */
  description: (query: string) => string;
  /** Whether this action should appear for the current query */
  isVisible: (query: string) => boolean;
  /** Execute the action. Returns entity ID on success. */
  execute: (query: string) => Promise<string>;
  /** Whether selecting the row should show an inline confirmation first. */
  requiresConfirmation?: boolean;
  /** Label shown during creation (e.g., "Opening agent composer...") */
  creatingLabel: string;
  /** Label shown on success (e.g., "Agent composer ready") */
  successLabel: string;
  /** Button text on success (e.g., "View Composer") */
  viewLabel: string;
  /** Navigate to the created entity */
  navigateTo: (entityId: string) => void;
}
