/**
 * Generic quick action state machine hook
 *
 * Manages the flow: idle → confirming → creating → success
 * Used by quick actions in the command palette and search interface.
 */

import { useState, useCallback } from "react";
import type { LucideIcon } from "lucide-react";

/**
 * State machine states for quick action flow
 */
export type QuickActionFlowState = "idle" | "confirming" | "creating" | "success";

/**
 * Quick action configuration
 */
export interface QuickAction {
  id: string;
  label: string;
  icon: LucideIcon;
  description: (query: string) => string;
  isVisible: (query: string) => boolean;
  execute: (query: string) => Promise<string>;
  requiresConfirmation?: boolean;
  creatingLabel: string;
  successLabel: string;
  viewLabel: string;
  navigateTo: (entityId: string) => void;
}

/**
 * Return type for useQuickActionFlow hook
 */
export interface UseQuickActionFlowReturn {
  flowState: QuickActionFlowState;
  createdEntityId: string | null;
  error: string | null;
  startConfirmation: () => void;
  confirm: (query: string) => Promise<void>;
  cancel: () => void;
  viewEntity: () => void;
  dismiss: () => void;
  isBlocking: boolean;
}

/**
 * Hook to manage quick action state machine flow
 *
 * State transitions:
 * - idle → (startConfirmation) → confirming
 * - confirming → (confirm) → creating → success
 * - confirming → (cancel) → idle
 * - creating → (error) → idle
 * - success → (dismiss/cancel/viewEntity) → idle
 */
export function useQuickActionFlow(action: QuickAction): UseQuickActionFlowReturn {
  const [flowState, setFlowState] = useState<QuickActionFlowState>("idle");
  const [createdEntityId, setCreatedEntityId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const startConfirmation = useCallback(() => {
    setFlowState("confirming");
    setError(null);
  }, []);

  const confirm = useCallback(
    async (query: string) => {
      setFlowState("creating");
      setError(null);

      try {
        const entityId = await action.execute(query);
        setCreatedEntityId(entityId);
        setFlowState("success");
      } catch (err) {
        // Extract error message
        let errorMessage: string;
        if (typeof err === "string") {
          errorMessage = err;
        } else if (err instanceof Error) {
          errorMessage = err.message;
        } else {
          errorMessage = "An unknown error occurred";
        }

        setError(errorMessage);
        setFlowState("idle");
      }
    },
    [action]
  );

  const cancel = useCallback(() => {
    setFlowState("idle");
    setCreatedEntityId(null);
  }, []);

  const viewEntity = useCallback(() => {
    if (createdEntityId) {
      action.navigateTo(createdEntityId);
    }
    setFlowState("idle");
    setCreatedEntityId(null);
  }, [action, createdEntityId]);

  const dismiss = useCallback(() => {
    setFlowState("idle");
    setCreatedEntityId(null);
  }, []);

  const isBlocking = flowState !== "idle";

  return {
    flowState,
    createdEntityId,
    error,
    startConfirmation,
    confirm,
    cancel,
    viewEntity,
    dismiss,
    isBlocking,
  };
}
